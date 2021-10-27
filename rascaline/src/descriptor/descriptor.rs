use std::collections::BTreeSet;

use indexmap::set::IndexSet;
use itertools::Itertools;
use ndarray::{Array1, Array2, ArrayView2, s};

use rayon::prelude::*;

use log::warn;

use crate::Error;
use super::{Indexes, IndexesBuilder, IndexValue};

#[derive(Clone)]
pub struct Descriptor {
    /// An array of samples.count() by features.count() values
    pub values: Array2<f64>,
    pub samples: Indexes,
    pub features: Indexes,
    /// Gradients of the descriptor with respect to one atomic position
    pub gradients: Option<Array2<f64>>,
    pub gradients_samples: Option<Indexes>,
}

impl Default for Descriptor {
    fn default() -> Self { Self::new() }
}

impl Descriptor {
    pub fn new() -> Descriptor {
        let indexes = IndexesBuilder::new(vec![]).finish();
        return Descriptor {
            values: Array2::zeros((0, 0)),
            samples: indexes.clone(),
            features: indexes,
            gradients: None,
            gradients_samples: None,
        }
    }

    pub fn prepare(
        &mut self,
        samples: Indexes,
        features: Indexes,
    ) {
        self.samples = samples;
        self.features = features;

        // resize the 'values' array if needed, and set the requested initial value
        let shape = (self.samples.count(), self.features.count());
        resize_and_reset(&mut self.values, shape);

        self.gradients = None;
        self.gradients_samples = None;
    }

    pub fn prepare_gradients(
        &mut self,
        samples: Indexes,
        gradients: Indexes,
        features: Indexes,
    ) {
        // basic sanity check
        assert_eq!(gradients.names().last(), Some(&"spatial"), "the last index of gradient should be spatial");

        self.samples = samples;
        self.features = features;

        // resize the 'values' array if needed, and set the requested initial value
        let shape = (self.samples.count(), self.features.count());
        resize_and_reset(&mut self.values, shape);

        let gradient_shape = (gradients.count(), self.features.count());
        self.gradients_samples = Some(gradients);

        if let Some(array) = &mut self.gradients {
            // resize the 'gradient' array if needed, and set the requested initial value
            resize_and_reset(array, gradient_shape);
        } else {
            // create a new gradient array
            let array = Array2::from_elem(gradient_shape, 0.0);
            self.gradients = Some(array);
        }
    }

    /// Make this descriptor dense along the given `variables`.
    ///
    /// This function "moves" the variables from the samples to the features,
    /// filling the new features with zeros if the corresponding sample is
    /// missing.
    ///
    /// The `requested` parameter defines which set of values taken by the
    /// `variables` should be part of the new features. If it is `None`, this is
    /// the set of values taken by the variables in the samples. Otherwise, it
    /// must be an array with one row for each new feature block, and one column
    /// for each variable.
    ///
    /// For example, take a descriptor containing two samples variables
    /// (`structure` and `species`) and two features (`n` and `l`). Starting
    /// with this descriptor:
    ///
    /// ```text
    ///                       +---+---+---+
    ///                       | n | 0 | 1 |
    ///                       +---+---+---+
    ///                       | l | 0 | 1 |
    /// +-----------+---------+===+===+===+
    /// | structure | species |           |
    /// +===========+=========+   +---+---+
    /// |     0     |    1    |   | 1 | 2 |
    /// +-----------+---------+   +---+---+
    /// |     0     |    6    |   | 3 | 4 |
    /// +-----------+---------+   +---+---+
    /// |     1     |    6    |   | 5 | 6 |
    /// +-----------+---------+   +---+---+
    /// |     1     |    8    |   | 7 | 8 |
    /// +-----------+---------+---+---+---+
    /// ```
    ///
    /// Calling `descriptor.densify(["species"], None)` will move `species` out
    /// of the samples and into the features, producing:
    ///
    /// ```text
    ///             +---------+-------+-------+-------+
    ///             | species |   1   |   6   |   8   |
    ///             +---------+---+---+---+---+---+---+
    ///             |    n    | 0 | 1 | 0 | 1 | 0 | 1 |
    ///             +---------+---+---+---+---+---+---+
    ///             |    l    | 0 | 1 | 0 | 1 | 0 | 1 |
    /// +-----------+=========+===+===+===+===+===+===+
    /// | structure |
    /// +===========+         +---+---+---+---+---+---+
    /// |     0     |         | 1 | 2 | 3 | 4 | 0 | 0 |
    /// +-----------+         +---+---+---+---+---+---+
    /// |     1     |         | 0 | 0 | 5 | 6 | 7 | 8 |
    /// +-----------+---------+---+---+---+---+---+---+
    /// ```
    ///
    /// Notice how there is only one row/sample for each structure now, and how
    /// each value for `species` have created a full block of features. Missing
    /// values (e.g. structure 0/species 8) have been filled with 0.
    #[time_graph::instrument(name="Descriptor::densify")]
    pub fn densify<'a>(
        &mut self,
        variables: &[&str],
        requested: impl Into<Option<ArrayView2<IndexValue, 'a>>>,
    ) -> Result<(), Error> {
        if variables.is_empty() || self.features.size() == 0 {
            return Ok(());
        }

        // if the user provided them, extract the set of values to use for the
        // new features.
        let requested_features = if let Some(requested) = requested.into() {
            let shape = requested.shape();
            if shape[1] != variables.len() {
                return Err(Error::InvalidParameter(format!(
                    "provided values in Descriptor::densify must match the \
                    variable size: expected {}, got {}", variables.len(), shape[1]
                )));
            }

            let mut features = BTreeSet::new();
            for value in requested.axis_iter(ndarray::Axis(0)) {
                features.insert(value.to_vec());
            }

            Some(features)
        } else {
            None
        };

        let variables_fmt = if variables.len() == 1 {
            variables[0].to_owned()
        } else {
            format!("({})", variables.join(", "))
        };

        let new_samples = remove_from_samples(&self.samples, variables)?;
        let new_gradients_samples = if let Some(ref gradients_samples) = self.gradients_samples {
            let new_gradients_samples = remove_from_samples(gradients_samples, variables)?;

            if new_gradients_samples.new_features != new_samples.new_features {
                panic!(
                    "gradient samples contains different values for {} than the \
                    samples themselves", variables_fmt
                );
            }

            Some(new_gradients_samples)
        } else {
            None
        };

        let requested_features = if let Some(requested_features) = requested_features {
            // check that all features in the dataset are part of the requested ones
            for f in &new_samples.new_features {
                if !requested_features.contains(f) {
                    warn!(
                        "{} takes the value {} in this descriptor, but it is \
                        not part of the requested features list",
                        variables_fmt, f.iter().map(|v| v.to_string()).join(",")
                    );
                }
            }
            requested_features
        } else {
            // if no features where requested by the user, use the list we have
            new_samples.new_features
        };

        // new feature indexes, add `variables` in the front. This transforms
        // something like [n, l, m] to [species_neighbor, n, l, m]; and fill it
        // with the corresponding values from `new_samples.new_features`,
        // duplicating the `[n, l, m]` block as needed
        let mut feature_names = variables.to_vec();
        feature_names.extend(self.features.names());
        let mut new_features = IndexesBuilder::new(feature_names);
        for new in requested_features {
            for feature in self.features.iter() {
                let mut new = new.clone();
                new.extend(feature);
                new_features.add(&new);
            }
        }
        let new_features = new_features.finish();

        let first_feature_tail = self.features.iter().next().expect("missing first feature").to_vec();
        let old_feature_size = self.features.count();

        // copy values themselves as needed
        let mut new_values = Array2::zeros((new_samples.samples.count(), new_features.count()));
        for changed in new_samples.mapping {
            let DensifiedIndex { old_sample_i, new_sample_i, variables } = changed;

            // find in which feature block we need to copy the data
            let mut first_feature = variables;
            first_feature.extend_from_slice(&first_feature_tail);

            // this can be None if the user requested a subset of all features
            if let Some(start) = new_features.position(&first_feature) {
                let stop = start + old_feature_size;

                let value = self.values.slice(s![old_sample_i, ..]);
                new_values.slice_mut(s![new_sample_i, start..stop]).assign(&value);
            }
        }

        if let Some(gradients) = &self.gradients {
            let new_gradients_samples = new_gradients_samples.expect("missing densified gradients");

            let mut new_gradients = Array2::zeros(
                (new_gradients_samples.samples.count(), new_features.count())
            );

            for changed in new_gradients_samples.mapping {
                let DensifiedIndex { old_sample_i, new_sample_i, variables } = changed;

                // find in which feature block we need to copy the data
                let mut first_feature = variables;
                first_feature.extend_from_slice(&first_feature_tail);
                // this can be None if the user requested a subset of all features
                if let Some(start) = new_features.position(&first_feature) {
                    let stop = start + old_feature_size;

                    let value = gradients.slice(s![old_sample_i, ..]);
                    new_gradients.slice_mut(s![new_sample_i, start..stop]).assign(&value);
                }
            }

            self.gradients = Some(new_gradients);
            self.gradients_samples = Some(new_gradients_samples.samples);
        }

        self.features = new_features;
        self.samples = new_samples.samples;
        self.values = new_values;

        return Ok(());
    }

    /// Compute the dot product between `self` and `other`, giving the resulting
    /// matrix in the `values` array of a new descriptor.
    ///
    /// The resulting dot product can then be used to compute kernels matrix,
    /// and gradients of these kernel matrix.
    ///
    /// The dot product is computed as if the user called
    /// `densify(reduce_across)` before, i.e.
    ///
    /// ```
    /// let dot = descriptor.dot(&other, &["variable"], (False, False));
    /// // is equivalent to
    /// descriptor.densify(&["variable"]);
    /// other.densify(&["variable"]);
    /// dot.values = descriptor.values.dot(other.values.t());
    /// ```
    ///
    /// The `gradients` parameter controls whether we are computing the dot
    /// product between values/gradient on the left hand side and
    /// values/gradients on the right hand side.
    ///
    /// ```
    /// // values / values dot product
    /// let dot = descriptor.dot(&other, &[], (False, False));
    /// // is equivalent to
    /// dot.values = descriptor.values.dot(other.values.t());
    ///
    /// // gradients / values dot product
    /// let dot = descriptor.dot(&other, &[], (True, False));
    /// // is equivalent to
    /// dot.values = descriptor.gradients.dot(other.values.t());
    ///
    /// // gradients / gradients dot product
    /// let dot = descriptor.dot(&other, &[], (True, True));
    /// // is equivalent to
    /// dot.values = descriptor.gradients.dot(other.gradients.t());
    /// ```
    #[time_graph::instrument]
    pub fn dot(
        &self,
        other: &Descriptor,
        options: DotOptions,
    ) -> Result<Descriptor, Error> {
        if self.features != other.features {
            return Err(Error::InvalidParameter(
                "descriptors have different features, the dot product between \
                them is not well defined".into()
            ));
        }

        for variable in options.reduce_across {
            if !self.samples.names().contains(variable) {
                return Err(Error::InvalidParameter(format!(
                    "'{}' does not appear on the left hand side samples for \
                    this dot product", variable
                )));
            }

            if !other.samples.names().contains(variable) {
                return Err(Error::InvalidParameter(format!(
                    "'{}' does not appear on the right hand side samples for \
                    this dot product", variable
                )));
            }
        }

        let rhs = &other.values;
        let rhs_samples = &other.samples;

        let removed_rhs = remove_from_samples(rhs_samples, options.reduce_across)?;
        let removed_lhs = remove_from_samples(&self.samples, options.reduce_across)?;
        let removed_grad = if options.gradients {
            let gradients_samples = self.gradients_samples.as_ref().ok_or_else(
                || Error::InvalidParameter(
                    "the left hand side descriptor does not contain gradient data, \
                    but the dot product requested it".into()
                )
            )?;

            Some(remove_from_samples(gradients_samples, options.reduce_across)?)
        } else {
            None
        };

        let mut output = Descriptor::new();
        if let Some(ref removed_grad) = removed_grad {
            output.prepare_gradients(removed_lhs.samples, removed_grad.samples.clone(), removed_rhs.samples);
        } else {
            output.prepare(removed_lhs.samples, removed_rhs.samples);
        }

        // transform from a DensifiedIndex identifying the new features as a
        // `Vec<IndexValue>` to a tuple identifying the new feature with a
        // single numeric id. This speeds up the double loop below by making the
        // `if feature_lhs != feature_rhs` comparison much faster.
        let mut features = std::collections::BTreeMap::new();
        let mut build_features_id = |mapping: Vec<DensifiedIndex>| {
            return mapping.into_iter().map(|densified| {
                let next_id = features.len();
                let feature_id = *features.entry(densified.variables).or_insert(next_id);

                return (densified.new_sample_i, densified.old_sample_i, feature_id);
            }).collect::<Vec<_>>();
        };

        #[derive(Clone)]
        struct DotIndexesPerRow {
            old_lhs: usize,
            rhs_indexes: Vec<(usize, usize)>,
        }


        let compute_dot_products_indexes = |lhs, rhs, n_rows| {
            let mut rows = Array1::from_elem(n_rows, Vec::new());

            for &(new_lhs, old_lhs, feature_lhs) in lhs {
                let mut rhs_indexes = Vec::new();
                for &(new_rhs, old_rhs, feature_rhs) in rhs {
                    // ensure that we are considering matching set of values from
                    // reduce_across (e.g. only consider dot product between
                    // matching `neighbor_species_1 / neighbor_species_2` values)
                    if feature_lhs != feature_rhs {
                        continue;
                    }

                    rhs_indexes.push((old_rhs, new_rhs));
                }
                rows[new_lhs].push(DotIndexesPerRow { old_lhs, rhs_indexes });
            }
            return rows;
        };

        let lhs_mapping = &build_features_id(removed_lhs.mapping);
        let rhs_mapping = &build_features_id(removed_rhs.mapping);


        // let n_cols = output.features.count();
        // let n_rows = output.samples.count();
        // let output_values = &mut output.values;

        let indexes = compute_dot_products_indexes(
            lhs_mapping, rhs_mapping, output.values.nrows()
        );
        ndarray::Zip::from(output.values.rows_mut())
            .and(&indexes)
            .par_for_each(|mut row, row_indexes| {
                for index in row_indexes {
                    let lhs_slice = self.values.slice(s![index.old_lhs, ..]);
                    for &(old_rhs, new_rhs) in &index.rhs_indexes {
                        let rhs_slice = rhs.slice(s![old_rhs, ..]);
                        row[new_rhs] += lhs_slice.dot(&rhs_slice);
                    }
                }
            });


        // let (sender, receiver) = crossbeam::channel::bounded(2 * rayon::current_num_threads());
        // crossbeam::thread::scope(|s| {
        //     s.spawn(move |_| {
        //         lhs_mapping.par_iter()
        //             .for_each(|&(new_lhs, old_lhs, feature_lhs)| {
        //                 let mut row = Array1::from_elem(n_cols, 0.0);
        //                 for &(new_rhs, old_rhs, feature_rhs) in rhs_mapping {
        //                     // ensure that we are considering matching set of
        //                     // values from reduce_across (e.g. only consider dot
        //                     // product between matching `neighbor_species_1 /
        //                     // neighbor_species_2` values)
        //                     if feature_lhs != feature_rhs {
        //                         continue;
        //                     }

        //                     let lhs_slice = self.values.slice(s![old_lhs, ..]);
        //                     let rhs_slice = rhs.slice(s![old_rhs, ..]);

        //                     row[new_rhs] += lhs_slice.dot(&rhs_slice);
        //                 }

        //                 sender.send((new_lhs, row)).expect("failed to send data");
        //             });
        //     });

        //     s.spawn(move |_| {
        //         for (i, values) in receiver {
        //             let mut row = output_values.slice_mut(s![i, ..]);
        //             row += &values;
        //         }
        //     });
        // }).expect("one of the thread panicked");


        // for &(new_lhs, old_lhs, feature_lhs) in &lhs_mapping {
        //     for &(new_rhs, old_rhs, feature_rhs) in &rhs_mapping {
        //         // ensure that we are considering matching set of values from
        //         // reduce_across (e.g. only consider dot product between
        //         // matching `neighbor_species_1/neighbor_species_2` values)
        //         if feature_lhs != feature_rhs {
        //             continue;
        //         }

        //         let lhs_slice = self.values.slice(s![old_lhs, ..]);
        //         let rhs_slice = rhs.slice(s![old_rhs, ..]);

        //         output.values[[new_lhs, new_rhs]] += lhs_slice.dot(&rhs_slice);

        //     }
        // }


        if let Some(removed_grad) = removed_grad {
            let gradient_mapping = &build_features_id(removed_grad.mapping);
            let output_gradients = output.gradients.as_mut().expect("missing gradient storage in output");
            let self_gradients = self.gradients.as_ref().expect("missing gradient data");

            let indexes = compute_dot_products_indexes(
                gradient_mapping, rhs_mapping, output_gradients.nrows()
            );

            ndarray::Zip::from(output_gradients.rows_mut())
                .and(&indexes)
                .par_for_each(|mut row, row_indexes| {
                    for index in row_indexes {
                        let lhs_slice = self_gradients.slice(s![index.old_lhs, ..]);
                        for &(old_rhs, new_rhs) in &index.rhs_indexes {
                            let rhs_slice = rhs.slice(s![old_rhs, ..]);
                            row[new_rhs] += lhs_slice.dot(&rhs_slice);
                        }
                    }
                });

            // let (sender, receiver) = crossbeam::channel::bounded(2 * rayon::current_num_threads());
            // crossbeam::thread::scope(|s| {
            //     s.spawn(move |_| {
            //         compute_dot_products_indexes(gradient_mapping, rhs_mapping)
            //             .par_iter()
            //             .for_each(|indexes| {
            //                 let lhs_slice = self_gradients.slice(s![indexes.old_lhs, ..]);
            //                 let rhs_slice = rhs.slice(s![indexes.old_rhs, ..]);

            //                 let dot = lhs_slice.dot(&rhs_slice);
            //                 sender.send((indexes.new, dot)).expect("failed to send data");
            //             });
            //     });

            //     s.spawn(move |_| {
            //         for ([i, j], value) in receiver {
            //             output_gradients[[i, j]] += value;
            //         }
            //     });
            // }).expect("one of the thread panicked");

            // crossbeam::thread::scope(|s| {
            //     s.spawn(move |_| {
            //         gradient_mapping.par_iter()
            //         .for_each(|&(new_lhs, old_lhs, feature_lhs)| {
            //             let mut row = Array1::from_elem(n_cols, 0.0);
            //             for &(new_rhs, old_rhs, feature_rhs) in rhs_mapping {
            //                 if feature_lhs != feature_rhs {
            //                     continue;
            //                 }

            //                 let lhs_slice = self_gradients.slice(s![old_lhs, ..]);
            //                 let rhs_slice = rhs.slice(s![old_rhs, ..]);

            //                 row[new_rhs] += lhs_slice.dot(&rhs_slice);
            //             }

            //             sender.send((new_lhs, row)).expect("failed to send data");
            //         });
            //     });

            //     s.spawn(move |_| {
            //         for (i, values) in receiver {
            //             let mut row = output_gradients.slice_mut(s![i, ..]);
            //             row += &values;
            //         }
            //     });
            // }).expect("one of the thread panicked");


            // for &(new_lhs, old_lhs, feature_lhs) in &grad_mapping {
            //     for &(new_rhs, old_rhs, feature_rhs) in rhs_mapping {
            //         if feature_lhs != feature_rhs {
            //             continue;
            //         }

            //         let lhs_slice = self_gradients.slice(s![old_lhs, ..]);
            //         let rhs_slice = rhs.slice(s![old_rhs, ..]);

            //         output_gradients[[new_lhs, new_rhs]] += lhs_slice.dot(&rhs_slice);
            //     }
            // }
        }


        // let mut lhs = self.clone();
        // let mut rhs = other.clone();

        // lhs.densify(options.reduce_across, None)?;
        // rhs.densify(options.reduce_across, None)?;

        // let mut output = Descriptor::new();
        // if options.gradients {
        //     output.prepare_gradients(lhs.samples, lhs.gradients_samples.unwrap(), rhs.samples);
        // } else {
        //     output.prepare(lhs.samples, rhs.samples);
        // }

        // output.values = lhs.values.dot(&rhs.values.t());
        // if options.gradients {
        //     let output_gradients = output.gradients.as_mut().expect("missing gradient storage in output");
        //     *output_gradients = lhs.gradients.unwrap().dot(&rhs.values.t());
        // }


        if options.normalize {
            let norm_lhs = compute_norm(&self.values, output.values.shape()[0], lhs_mapping);
            let norm_rhs = compute_norm(rhs, output.values.shape()[1], rhs_mapping);

            output.values.indexed_iter_mut().for_each(|((i, j), value)| {
                *value /= norm_lhs[i] * norm_rhs[j];
            });

            if let Some(ref mut gradients) = output.gradients {
                let gradients_samples = output.gradients_samples.as_ref().expect("missing gradient storage");

                // we assume the final two gradient samples variables are
                // atom/neighbor and then spatial
                let gradient_samples_size = gradients_samples.size();
                assert_eq!(gradient_samples_size, output.samples.size() + 2);
                assert_eq!(gradients_samples.names()[gradient_samples_size - 1], "spatial");

                let mut norm_grad = Array1::from_elem(gradients_samples.count(), 0.0);
                for (i_gradient, gradient_sample) in gradients_samples.iter().enumerate() {
                    let sample = &gradient_sample[..(gradient_samples_size - 2)];
                    let i_value = output.samples.position(sample)
                        .expect("this gradient sample does not correspond to a value sample");

                    norm_grad[i_gradient] = norm_lhs[i_value];
                }

                gradients.indexed_iter_mut().for_each(|((i, j), value)| {
                    *value /= norm_grad[i] * norm_rhs[j];
                });

            }
        }

        return Ok(output);
    }
}

fn resize_and_reset(array: &mut Array2<f64>, shape: (usize, usize)) {
    // extract data by replacing array with a temporary value
    let mut tmp = Array2::zeros((0, 0));
    std::mem::swap(array, &mut tmp);

    let mut data = tmp.into_raw_vec();
    data.resize(shape.0 * shape.1, 0.0);

    let mut values = Array2::from_shape_vec(shape, data).expect("wrong array shape");
    values.fill(0.0);
    let _replaced = std::mem::replace(array, values);
}

/// A `DensifiedIndex` contains all the information to reconstruct the new
/// position of the values/gradients associated with a single sample in the
/// initial descriptor
#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq)]
struct DensifiedIndex {
    /// Index of the old sample (respectively gradient sample) in the value
    /// (respectively gradients) array
    old_sample_i: usize,
    /// Index of the new sample (respectively gradient sample) in the value
    /// (respectively gradients) array
    new_sample_i: usize,
    /// Values of the variables in the old descriptor. These are part of the
    /// samples in the old descriptor; but part of the features in the new one.
    variables: Vec<IndexValue>,
}

/// Results of removing a set of variables from samples
struct RemovedSamples {
    /// New samples, without the variables
    samples: Indexes,
    /// Values taken by the variables in the original samples
    new_features: BTreeSet<Vec<IndexValue>>,
    /// Information about all data that needs to be moved
    mapping: Vec<DensifiedIndex>,
}

/// Remove the given `variables` from the `samples`, returning the updated
/// `samples` and a set of all the values taken by the removed variables.
fn remove_from_samples(samples: &Indexes, variables: &[&str]) -> Result<RemovedSamples, Error> {
    let mut variables_positions = Vec::new();
    for v in variables {
        let position = samples.names().iter().position(|name| name == v);
        if let Some(position) = position {
            variables_positions.push(position);
        } else {
            return Err(Error::InvalidParameter(format!(
                "can not densify along '{}' which is not present in the samples: [{}]",
                    v, samples.names().join(", ")
            )))
        }
    }

    let mut mapping = Vec::new();

    // collect all different indexes in maps. Assuming we are densifying
    // along the first index, we want to convert [[2, 3, 0], [1, 3, 0]]
    // to [[3, 0]].
    let mut new_samples = IndexSet::new();
    let mut new_features = BTreeSet::new();

    for (old_sample_i, sample) in samples.iter().enumerate() {
        let mut new_feature = Vec::new();
        for &i in &variables_positions {
            new_feature.push(sample[i]);
        }
        new_features.insert(new_feature.clone());

        let mut new_sample = sample.to_vec();
        // sort and reverse the indexes to ensure the all the calls to `remove`
        // are valid
        for &i in variables_positions.iter().sorted().rev() {
            new_sample.remove(i);
        }
        let (new_sample_i, _) = new_samples.insert_full(new_sample);

        let densified = DensifiedIndex {
            old_sample_i: old_sample_i,
            new_sample_i: new_sample_i,
            variables: new_feature,
        };
        mapping.push(densified);
    }

    let names = samples.names()
        .iter()
        .filter(|&name| !variables.contains(name))
        .copied()
        .collect();
    let mut builder = IndexesBuilder::new(names);
    for sample in new_samples {
        builder.add(&sample);
    }

    return Ok(RemovedSamples {
        samples: builder.finish(),
        new_features: new_features,
        mapping: mapping,
    });
}

/// Compute the 2-norm of each row in the values array using the provided
/// mapping. This is a helper function for `Descriptor::dot`
fn compute_norm(values: &Array2<f64>, size: usize, mapping: &[(usize, usize, usize)]) -> Array1<f64> {
    let mut output = Array1::from_elem(size, 0.0);

    for &(new_lhs, old_lhs, feature_lhs) in mapping {
        for &(new_rhs, old_rhs, feature_rhs) in mapping {
            // only consider values on the diagonal
            if new_lhs != new_rhs {
                continue;
            }

            // ensure that we are considering matching set of values from
            // reduce_across (e.g. only consider dot product between
            // matching `neighbor_species_1/neighbor_species_2` values)
            if feature_lhs != feature_rhs {
                continue;
            }

            let lhs_slice = values.slice(s![old_lhs, ..]);
            let rhs_slice = values.slice(s![old_rhs, ..]);

            output[new_lhs] += lhs_slice.dot(&rhs_slice);
        }
    }

    output.iter_mut().for_each(|v| *v = f64::sqrt(*v));

    return output;
}

#[derive(Debug, Clone)]
pub struct DotOptions<'a> {
    pub reduce_across: &'a [&'a str],
    pub normalize: bool,
    pub gradients: bool,
}

impl<'a> Default for DotOptions<'a> {
    fn default() -> DotOptions<'a> {
        DotOptions {
            reduce_across: &[],
            normalize: false,
            gradients: false,
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::test_utils::test_systems;
    use crate::descriptor::{TwoBodiesSpeciesSamples, StructureSpeciesSamples, SamplesBuilder};
    use ndarray::array;

    fn dummy_features() -> Indexes {
        let mut features = IndexesBuilder::new(vec!["foo", "bar"]);
        features.add(&[IndexValue::from(0), IndexValue::from(-1)]);
        features.add(&[IndexValue::from(4), IndexValue::from(-2)]);
        features.add(&[IndexValue::from(1), IndexValue::from(-5)]);
        return features.finish();
    }

    // small helper function to create IndexValue
    fn v(i: i32) -> IndexValue { IndexValue::from(i) }

    #[test]
    fn prepare() {
        let mut descriptor = Descriptor::new();

        let mut systems = test_systems(&["water", "CH"]);
        let features = dummy_features();
        let samples = StructureSpeciesSamples.samples(&mut systems).unwrap();
        descriptor.prepare(samples, features);


        assert_eq!(descriptor.values.shape(), [4, 3]);

        assert_eq!(descriptor.samples.names(), ["structure", "species"]);
        assert_eq!(descriptor.samples[0], [v(0), v(1)]);
        assert_eq!(descriptor.samples[1], [v(0), v(123456)]);
        assert_eq!(descriptor.samples[2], [v(1), v(1)]);
        assert_eq!(descriptor.samples[3], [v(1), v(6)]);

        assert!(descriptor.gradients.is_none());
    }

    #[test]
    fn prepare_gradients() {
        let mut descriptor = Descriptor::new();

        let mut systems = test_systems(&["water", "CH"]);
        let features = dummy_features();
        let (samples, gradients) = StructureSpeciesSamples.with_gradients(&mut systems).unwrap();
        descriptor.prepare_gradients(samples, gradients.unwrap(), features);

        let gradients = descriptor.gradients.unwrap();
        assert_eq!(gradients.shape(), [15, 3]);

        let gradients_samples = descriptor.gradients_samples.as_ref().unwrap();
        assert_eq!(gradients_samples.names(), ["structure", "species", "atom", "spatial"]);

        let expected = [
            [v(0), v(1), v(1)],
            [v(0), v(1), v(2)],
            [v(0), v(123456), v(0)],
            [v(1), v(1), v(0)],
            [v(1), v(6), v(1)]
        ];
        // use a loop to simplify checking the spatial dimension
        for (i, &value) in expected.iter().enumerate() {
            assert_eq!(gradients_samples[3 * i][..3], value);
            assert_eq!(gradients_samples[3 * i][3], v(0));

            assert_eq!(gradients_samples[3 * i + 1][..3], value);
            assert_eq!(gradients_samples[3 * i + 1][3], v(1));

            assert_eq!(gradients_samples[3 * i + 2][..3], value);
            assert_eq!(gradients_samples[3 * i + 2][3], v(2));
        }
    }

    #[test]
    fn densify_single_variable() {
        let mut descriptor = Descriptor::new();

        let mut systems = test_systems(&["water", "CH"]);
        let features = dummy_features();
        let (samples, gradients) = StructureSpeciesSamples.with_gradients(&mut systems).unwrap();
        descriptor.prepare_gradients(samples, gradients.unwrap(), features);

        descriptor.values.assign(&array![
            [1.0, 2.0, 3.0],
            [4.0, 5.0, 6.0],
            [7.0, 8.0, 9.0],
            [10.0, 11.0, 12.0],
        ]);

        let gradients = descriptor.gradients.as_mut().unwrap();
        gradients.assign(&array![
            [1.0, 2.0, 3.0], [0.1, 0.2, 0.3], [-1.0, -2.0, -3.0],
            [4.0, 5.0, 6.0], [0.4, 0.5, 0.6], [-4.0, -5.0, -6.0],
            [7.0, 8.0, 9.0], [0.7, 0.8, 0.9], [-7.0, -8.0, -9.0],
            [10.0, 11.0, 12.0], [0.10, 0.11, 0.12], [-10.0, -11.0, -12.0],
            [13.0, 14.0, 15.0], [0.13, 0.14, 0.15], [-13.0, -14.0, -15.0],
        ]);

        // where the magic happens
        descriptor.densify(&["species"], None).unwrap();

        assert_eq!(descriptor.features.names(), ["species", "foo", "bar"]);
        assert_eq!(descriptor.features[0], [v(1), v(0), v(-1)]);
        assert_eq!(descriptor.features[1], [v(1), v(4), v(-2)]);
        assert_eq!(descriptor.features[2], [v(1), v(1), v(-5)]);
        assert_eq!(descriptor.features[3], [v(6), v(0), v(-1)]);
        assert_eq!(descriptor.features[4], [v(6), v(4), v(-2)]);
        assert_eq!(descriptor.features[5], [v(6), v(1), v(-5)]);
        assert_eq!(descriptor.features[6], [v(123456), v(0), v(-1)]);
        assert_eq!(descriptor.features[7], [v(123456), v(4), v(-2)]);
        assert_eq!(descriptor.features[8], [v(123456), v(1), v(-5)]);

        assert_eq!(descriptor.values.shape(), [2, 9]);
        assert_eq!(descriptor.samples.names(), ["structure"]);
        assert_eq!(descriptor.samples[0], [v(0)]);
        assert_eq!(descriptor.samples[1], [v(1)]);

        assert_eq!(descriptor.values, array![
            [/* H */ 1.0, 2.0, 3.0, /* C */ 0.0, 0.0, 0.0,    /* O */ 4.0, 5.0, 6.0],
            [/* H */ 7.0, 8.0, 9.0, /* C */ 10.0, 11.0, 12.0, /* O */ 0.0, 0.0, 0.0],
        ]);

        let gradients = descriptor.gradients.as_ref().unwrap();
        assert_eq!(gradients.shape(), [15, 9]);
        let gradients_samples = descriptor.gradients_samples.as_ref().unwrap();
        assert_eq!(gradients_samples.names(), ["structure", "atom", "spatial"]);

        let expected = [
            [v(0), v(1)],
            [v(0), v(2)],
            [v(0), v(0)],
            [v(1), v(0)],
            [v(1), v(1)]
        ];
        // use a loop to simplify checking the spatial dimension
        for (i, &value) in expected.iter().enumerate() {
            assert_eq!(gradients_samples[3 * i][..2], value);
            assert_eq!(gradients_samples[3 * i][2], v(0));

            assert_eq!(gradients_samples[3 * i + 1][..2], value);
            assert_eq!(gradients_samples[3 * i + 1][2], v(1));

            assert_eq!(gradients_samples[3 * i + 2][..2], value);
            assert_eq!(gradients_samples[3 * i + 2][2], v(2));
        }

        assert_eq!(*gradients, array![
            [/*H*/ 1.0, 2.0, 3.0,       /*C*/ 0.0, 0.0, 0.0,        /*O*/ 0.0, 0.0, 0.0],
            [/*H*/ 0.1, 0.2, 0.3,       /*C*/ 0.0, 0.0, 0.0,        /*O*/ 0.0, 0.0, 0.0],
            [/*H*/ -1.0, -2.0, -3.0,    /*C*/ 0.0, 0.0, 0.0,        /*O*/ 0.0, 0.0, 0.0],
            [/*H*/ 4.0, 5.0, 6.0,       /*C*/ 0.0, 0.0, 0.0,        /*O*/ 0.0, 0.0, 0.0],
            [/*H*/ 0.4, 0.5, 0.6,       /*C*/ 0.0, 0.0, 0.0,        /*O*/ 0.0, 0.0, 0.0],
            [/*H*/ -4.0, -5.0, -6.0,    /*C*/ 0.0, 0.0, 0.0,        /*O*/ 0.0, 0.0, 0.0],
            [/*H*/ 0.0, 0.0, 0.0,       /*C*/ 0.0, 0.0, 0.0,        /*O*/ 7.0, 8.0, 9.0],
            [/*H*/ 0.0, 0.0, 0.0,       /*C*/ 0.0, 0.0, 0.0,        /*O*/ 0.7, 0.8, 0.9],
            [/*H*/ 0.0, 0.0, 0.0,       /*C*/ 0.0, 0.0, 0.0,        /*O*/ -7.0, -8.0, -9.0],
            [/*H*/ 10.0, 11.0, 12.0,    /*C*/ 0.0, 0.0, 0.0,        /*O*/ 0.0, 0.0, 0.0],
            [/*H*/ 0.1, 0.11, 0.12,     /*C*/ 0.0, 0.0, 0.0,        /*O*/ 0.0, 0.0, 0.0],
            [/*H*/ -10.0, -11.0, -12.0, /*C*/ 0.0, 0.0, 0.0,        /*O*/ 0.0, 0.0, 0.0],
            [/*H*/ 0.0, 0.0, 0.0,       /*C*/ 13.0, 14.0, 15.0,     /*O*/ 0.0, 0.0, 0.0],
            [/*H*/ 0.0, 0.0, 0.0,       /*C*/ 0.13, 0.14, 0.15,     /*O*/ 0.0, 0.0, 0.0],
            [/*H*/ 0.0, 0.0, 0.0,       /*C*/ -13.0, -14.0, -15.0,  /*O*/ 0.0, 0.0, 0.0],
        ]);
    }

    #[test]
    fn densify_single_variable_user_values() {
        let mut descriptor = Descriptor::new();

        let mut systems = test_systems(&["water", "CH"]);
        let features = dummy_features();
        let (samples, gradients) = StructureSpeciesSamples.with_gradients(&mut systems).unwrap();
        descriptor.prepare_gradients(samples, gradients.unwrap(), features);

        descriptor.values.assign(&array![
            [1.0, 2.0, 3.0],
            [4.0, 5.0, 6.0],
            [7.0, 8.0, 9.0],
            [10.0, 11.0, 12.0],
        ]);

        let gradients = descriptor.gradients.as_mut().unwrap();
        gradients.assign(&array![
            [1.0, 2.0, 3.0], [0.1, 0.2, 0.3], [-1.0, -2.0, -3.0],
            [4.0, 5.0, 6.0], [0.4, 0.5, 0.6], [-4.0, -5.0, -6.0],
            [7.0, 8.0, 9.0], [0.7, 0.8, 0.9], [-7.0, -8.0, -9.0],
            [10.0, 11.0, 12.0], [0.10, 0.11, 0.12], [-10.0, -11.0, -12.0],
            [13.0, 14.0, 15.0], [0.13, 0.14, 0.15], [-13.0, -14.0, -15.0],
        ]);

        let requested = Array2::from_shape_vec([3, 1], vec![
            v(6), v(12), v(123456)
        ]).unwrap();
        descriptor.densify(&["species"], requested.view()).unwrap();

        assert_eq!(descriptor.features.names(), ["species", "foo", "bar"]);
        assert_eq!(descriptor.features[0], [v(6), v(0), v(-1)]);
        assert_eq!(descriptor.features[1], [v(6), v(4), v(-2)]);
        assert_eq!(descriptor.features[2], [v(6), v(1), v(-5)]);
        assert_eq!(descriptor.features[3], [v(12), v(0), v(-1)]);
        assert_eq!(descriptor.features[4], [v(12), v(4), v(-2)]);
        assert_eq!(descriptor.features[5], [v(12), v(1), v(-5)]);
        assert_eq!(descriptor.features[6], [v(123456), v(0), v(-1)]);
        assert_eq!(descriptor.features[7], [v(123456), v(4), v(-2)]);
        assert_eq!(descriptor.features[8], [v(123456), v(1), v(-5)]);

        assert_eq!(descriptor.values.shape(), [2, 9]);
        assert_eq!(descriptor.samples.names(), ["structure"]);
        assert_eq!(descriptor.samples[0], [v(0)]);
        assert_eq!(descriptor.samples[1], [v(1)]);

        assert_eq!(descriptor.values, array![
            [/* C */ 0.0, 0.0, 0.0,    /* missing */ 0.0, 0.0, 0.0, /* O */ 4.0, 5.0, 6.0],
            [/* C */ 10.0, 11.0, 12.0, /* missing */ 0.0, 0.0, 0.0, /* O */ 0.0, 0.0, 0.0],
        ]);

        let gradients = descriptor.gradients.as_ref().unwrap();
        assert_eq!(*gradients, array![
            [/*C*/ 0.0, 0.0, 0.0,        /*missing*/ 0.0, 0.0, 0.0, /*O*/ 0.0, 0.0, 0.0],
            [/*C*/ 0.0, 0.0, 0.0,        /*missing*/ 0.0, 0.0, 0.0, /*O*/ 0.0, 0.0, 0.0],
            [/*C*/ 0.0, 0.0, 0.0,        /*missing*/ 0.0, 0.0, 0.0, /*O*/ 0.0, 0.0, 0.0],
            [/*C*/ 0.0, 0.0, 0.0,        /*missing*/ 0.0, 0.0, 0.0, /*O*/ 0.0, 0.0, 0.0],
            [/*C*/ 0.0, 0.0, 0.0,        /*missing*/ 0.0, 0.0, 0.0, /*O*/ 0.0, 0.0, 0.0],
            [/*C*/ 0.0, 0.0, 0.0,        /*missing*/ 0.0, 0.0, 0.0, /*O*/ 0.0, 0.0, 0.0],
            [/*C*/ 0.0, 0.0, 0.0,        /*missing*/ 0.0, 0.0, 0.0, /*O*/ 7.0, 8.0, 9.0],
            [/*C*/ 0.0, 0.0, 0.0,        /*missing*/ 0.0, 0.0, 0.0, /*O*/ 0.7, 0.8, 0.9],
            [/*C*/ 0.0, 0.0, 0.0,        /*missing*/ 0.0, 0.0, 0.0, /*O*/ -7.0, -8.0, -9.0],
            [/*C*/ 0.0, 0.0, 0.0,        /*missing*/ 0.0, 0.0, 0.0, /*O*/ 0.0, 0.0, 0.0],
            [/*C*/ 0.0, 0.0, 0.0,        /*missing*/ 0.0, 0.0, 0.0, /*O*/ 0.0, 0.0, 0.0],
            [/*C*/ 0.0, 0.0, 0.0,        /*missing*/ 0.0, 0.0, 0.0, /*O*/ 0.0, 0.0, 0.0],
            [/*C*/ 13.0, 14.0, 15.0,     /*missing*/ 0.0, 0.0, 0.0, /*O*/ 0.0, 0.0, 0.0],
            [/*C*/ 0.13, 0.14, 0.15,     /*missing*/ 0.0, 0.0, 0.0, /*O*/ 0.0, 0.0, 0.0],
            [/*C*/ -13.0, -14.0, -15.0,  /*missing*/ 0.0, 0.0, 0.0, /*O*/ 0.0, 0.0, 0.0],
        ]);
    }

    #[test]
    fn densify_multiple_variables() {
        let mut descriptor = Descriptor::new();

        let mut systems = test_systems(&["water"]);
        let features = dummy_features();
        let (samples, gradients) = TwoBodiesSpeciesSamples::new(3.0).with_gradients(&mut systems).unwrap();
        descriptor.prepare_gradients(samples, gradients.unwrap(), features);

        descriptor.values.assign(&array![
            // H channel around O
            [1.0, 2.0, 3.0],
            // H channel around H1
            [4.0, 5.0, 6.0],
            // O channel around H1
            [7.0, 8.0, 9.0],
            // H channel around H2
            [10.0, 11.0, 12.0],
            // O channel around H2
            [13.0, 14.0, 15.0],
        ]);

        let gradients = descriptor.gradients.as_mut().unwrap();
        gradients.assign(&array![
            // H channel around O, derivatives w.r.t. O
            [1.0, 0.1, -1.0], [2.0, 0.2, -2.0], [3.0, 0.3, -3.0],
            // H channel around O, derivatives w.r.t. H1
            [4.0, 0.4, -4.0], [5.0, 0.5, -5.0], [6.0, 0.6, -6.0],
            // H channel around O, derivatives w.r.t. H2
            [7.0, 0.7, -7.0], [8.0, 0.8, -8.0], [9.0, 0.9, -9.0],
            // H channel around H1, derivatives w.r.t. H1
            [10.0, 0.10, -10.0], [11.0, 0.11, -11.0], [12.0, 0.12, -12.0],
            // H channel around H1, derivatives w.r.t. H2
            [13.0, 0.13, -13.0], [14.0, 0.14, -14.0], [15.0, 0.15, -15.0],
            // O channel around H1, derivatives w.r.t. H1
            [16.0, 0.16, -16.0], [17.0, 0.17, -17.0], [18.0, 0.18, -18.0],
            // O channel around H1, derivatives w.r.t. O
            [19.0, 0.19, -19.0], [20.0, 0.20, -20.0], [21.0, 0.21, -21.0],
            // H channel around H2, derivatives w.r.t. H2
            [22.0, 0.22, -22.0], [23.0, 0.23, -23.0], [24.0, 0.24, -24.0],
            // H channel around H2, derivatives w.r.t. H1
            [25.0, 0.25, -25.0], [26.0, 0.26, -26.0], [27.0, 0.27, -27.0],
            // O channel around H2, derivatives w.r.t. H2
            [28.0, 0.28, -28.0], [29.0, 0.29, -29.0], [30.0, 0.30, -30.0],
            // O channel around H2, derivatives w.r.t. O
            [31.0, 0.31, -31.0], [32.0, 0.32, -32.0], [33.0, 0.33, -33.0],
        ]);

        // where the magic happens
        descriptor.densify(&["species_center", "species_neighbor"], None).unwrap();

        assert_eq!(descriptor.values.shape(), [3, 9]);
        assert_eq!(descriptor.samples.names(), ["structure", "center"]);
        assert_eq!(descriptor.samples[0], [v(0), v(0)]);
        assert_eq!(descriptor.samples[1], [v(0), v(1)]);
        assert_eq!(descriptor.samples[2], [v(0), v(2)]);

        assert_eq!(descriptor.features.names(), ["species_center", "species_neighbor", "foo", "bar"]);
        assert_eq!(descriptor.features[0], [v(1), v(1), v(0), v(-1)]);
        assert_eq!(descriptor.features[1], [v(1), v(1), v(4), v(-2)]);
        assert_eq!(descriptor.features[2], [v(1), v(1), v(1), v(-5)]);
        assert_eq!(descriptor.features[3], [v(1), v(123456), v(0), v(-1)]);
        assert_eq!(descriptor.features[4], [v(1), v(123456), v(4), v(-2)]);
        assert_eq!(descriptor.features[5], [v(1), v(123456), v(1), v(-5)]);
        assert_eq!(descriptor.features[6], [v(123456), v(1), v(0), v(-1)]);
        assert_eq!(descriptor.features[7], [v(123456), v(1), v(4), v(-2)]);
        assert_eq!(descriptor.features[8], [v(123456), v(1), v(1), v(-5)]);

        assert_eq!(descriptor.values, array![
            /*    H-H                    H-O                  O-H      */
            // O in water
            [0.0, 0.0, 0.0,    /**/ 0.0, 0.0, 0.0,    /**/ 1.0, 2.0, 3.0],
            // H1 in water
            [4.0, 5.0, 6.0,    /**/ 7.0, 8.0, 9.0,    /**/ 0.0, 0.0, 0.0],
            // H2 in water
            [10.0, 11.0, 12.0, /**/ 13.0, 14.0, 15.0, /**/ 0.0, 0.0, 0.0],
        ]);

        let gradients = descriptor.gradients.as_ref().unwrap();
        assert_eq!(gradients.shape(), [27, 9]);
        let gradients_samples = descriptor.gradients_samples.as_ref().unwrap();
        assert_eq!(gradients_samples.names(), ["structure", "center", "neighbor", "spatial"]);

        let expected = [
            [v(0), v(0), v(0)],
            [v(0), v(0), v(1)],
            [v(0), v(0), v(2)],
            [v(0), v(1), v(1)],
            [v(0), v(1), v(2)],
            [v(0), v(1), v(0)],
            [v(0), v(2), v(2)],
            [v(0), v(2), v(1)],
            [v(0), v(2), v(0)],
        ];
        // use a loop to simplify checking the spatial dimension
        for (i, &value) in expected.iter().enumerate() {
            assert_eq!(gradients_samples[3 * i][..3], value);
            assert_eq!(gradients_samples[3 * i][3], v(0));

            assert_eq!(gradients_samples[3 * i + 1][..3], value);
            assert_eq!(gradients_samples[3 * i + 1][3], v(1));

            assert_eq!(gradients_samples[3 * i + 2][..3], value);
            assert_eq!(gradients_samples[3 * i + 2][3], v(2));
        }

        assert_eq!(*gradients, array![
            /*    H-H                  H-O                  O-H       */
            // O in water, derivatives w.r.t. O
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,       1.0, 0.1, -1.0],
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,       2.0, 0.2, -2.0],
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,       3.0, 0.3, -3.0],
            // O in water, derivatives w.r.t. H1
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,       4.0, 0.4, -4.0],
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,       5.0, 0.5, -5.0],
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,       6.0, 0.6, -6.0],
            // O in water, derivatives w.r.t. H2
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,       7.0, 0.7, -7.0],
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,       8.0, 0.8, -8.0],
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,       9.0, 0.9, -9.0],
            // H1 in water, derivatives w.r.t. H1
            [10.0, 0.10, -10.0,    16.0, 0.16, -16.0,   0.0, 0.0, 0.0],
            [11.0, 0.11, -11.0,    17.0, 0.17, -17.0,   0.0, 0.0, 0.0],
            [12.0, 0.12, -12.0,    18.0, 0.18, -18.0,   0.0, 0.0, 0.0],
            // H1 in water, derivatives w.r.t. H2
            [13.0, 0.13, -13.0,    0.0, 0.0, 0.0,       0.0, 0.0, 0.0],
            [14.0, 0.14, -14.0,    0.0, 0.0, 0.0,       0.0, 0.0, 0.0],
            [15.0, 0.15, -15.0,    0.0, 0.0, 0.0,       0.0, 0.0, 0.0],
            // H1 in water, derivatives w.r.t. O
            [0.0, 0.0, 0.0,        19.0, 0.19, -19.0,   0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0,        20.0, 0.20, -20.0,   0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0,        21.0, 0.21, -21.0,   0.0, 0.0, 0.0],
            // H2 in water, derivatives w.r.t. H2
            [22.0, 0.22, -22.0,    28.0, 0.28, -28.0,   0.0, 0.0, 0.0],
            [23.0, 0.23, -23.0,    29.0, 0.29, -29.0,   0.0, 0.0, 0.0],
            [24.0, 0.24, -24.0,    30.0, 0.30, -30.0,   0.0, 0.0, 0.0],
            // H2 in water, derivatives w.r.t. H1
            [25.0, 0.25, -25.0,    0.0, 0.0, 0.0,       0.0, 0.0, 0.0],
            [26.0, 0.26, -26.0,    0.0, 0.0, 0.0,       0.0, 0.0, 0.0],
            [27.0, 0.27, -27.0,    0.0, 0.0, 0.0,       0.0, 0.0, 0.0],
            // H2 in water, derivatives w.r.t. O
            [0.0, 0.0, 0.0,        31.0, 0.31, -31.0,   0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0,        32.0, 0.32, -32.0,   0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0,        33.0, 0.33, -33.0,   0.0, 0.0, 0.0],
        ]);
    }

    #[test]
    fn densify_multiple_variables_user_values() {
        let mut descriptor = Descriptor::new();

        let mut systems = test_systems(&["water"]);
        let features = dummy_features();
        let (samples, gradients) = TwoBodiesSpeciesSamples::new(3.0).with_gradients(&mut systems).unwrap();
        descriptor.prepare_gradients(samples, gradients.unwrap(), features);

        descriptor.values.assign(&array![
            // H channel around O
            [1.0, 2.0, 3.0],
            // H channel around H1
            [4.0, 5.0, 6.0],
            // O channel around H1
            [7.0, 8.0, 9.0],
            // H channel around H2
            [10.0, 11.0, 12.0],
            // O channel around H2
            [13.0, 14.0, 15.0],
        ]);

        let gradients = descriptor.gradients.as_mut().unwrap();
        gradients.assign(&array![
            // H channel around O, derivatives w.r.t. O
            [1.0, 0.1, -1.0], [2.0, 0.2, -2.0], [3.0, 0.3, -3.0],
            // H channel around O, derivatives w.r.t. H1
            [4.0, 0.4, -4.0], [5.0, 0.5, -5.0], [6.0, 0.6, -6.0],
            // H channel around O, derivatives w.r.t. H2
            [7.0, 0.7, -7.0], [8.0, 0.8, -8.0], [9.0, 0.9, -9.0],
            // H channel around H1, derivatives w.r.t. H1
            [10.0, 0.10, -10.0], [11.0, 0.11, -11.0], [12.0, 0.12, -12.0],
            // H channel around H1, derivatives w.r.t. H2
            [13.0, 0.13, -13.0], [14.0, 0.14, -14.0], [15.0, 0.15, -15.0],
            // O channel around H1, derivatives w.r.t. H1
            [16.0, 0.16, -16.0], [17.0, 0.17, -17.0], [18.0, 0.18, -18.0],
            // O channel around H1, derivatives w.r.t. O
            [19.0, 0.19, -19.0], [20.0, 0.20, -20.0], [21.0, 0.21, -21.0],
            // H channel around H2, derivatives w.r.t. H2
            [22.0, 0.22, -22.0], [23.0, 0.23, -23.0], [24.0, 0.24, -24.0],
            // H channel around H2, derivatives w.r.t. H1
            [25.0, 0.25, -25.0], [26.0, 0.26, -26.0], [27.0, 0.27, -27.0],
            // O channel around H2, derivatives w.r.t. H2
            [28.0, 0.28, -28.0], [29.0, 0.29, -29.0], [30.0, 0.30, -30.0],
            // O channel around H2, derivatives w.r.t. O
            [31.0, 0.31, -31.0], [32.0, 0.32, -32.0], [33.0, 0.33, -33.0],
        ]);

        let requested = Array2::from_shape_vec([3, 2], vec![
            v(1), v(1),       // H-H
            v(6), v(1),       // missing
            v(123456), v(1),  // O-H
        ]).unwrap();
        descriptor.densify(&["species_center", "species_neighbor"], requested.view()).unwrap();

        assert_eq!(descriptor.values.shape(), [3, 9]);
        assert_eq!(descriptor.samples.names(), ["structure", "center"]);
        assert_eq!(descriptor.samples[0], [v(0), v(0)]);
        assert_eq!(descriptor.samples[1], [v(0), v(1)]);
        assert_eq!(descriptor.samples[2], [v(0), v(2)]);

        assert_eq!(descriptor.features.names(), ["species_center", "species_neighbor", "foo", "bar"]);
        assert_eq!(descriptor.features[0], [v(1), v(1), v(0), v(-1)]);
        assert_eq!(descriptor.features[1], [v(1), v(1), v(4), v(-2)]);
        assert_eq!(descriptor.features[2], [v(1), v(1), v(1), v(-5)]);
        assert_eq!(descriptor.features[3], [v(6), v(1), v(0), v(-1)]);
        assert_eq!(descriptor.features[4], [v(6), v(1), v(4), v(-2)]);
        assert_eq!(descriptor.features[5], [v(6), v(1), v(1), v(-5)]);
        assert_eq!(descriptor.features[6], [v(123456), v(1), v(0), v(-1)]);
        assert_eq!(descriptor.features[7], [v(123456), v(1), v(4), v(-2)]);
        assert_eq!(descriptor.features[8], [v(123456), v(1), v(1), v(-5)]);

        assert_eq!(descriptor.values, array![
            /*    H-H                 missing              O-H      */
            // O in water
            [0.0, 0.0, 0.0,    /**/ 0.0, 0.0, 0.0, /**/ 1.0, 2.0, 3.0],
            // H1 in water
            [4.0, 5.0, 6.0,    /**/ 0.0, 0.0, 0.0, /**/ 0.0, 0.0, 0.0],
            // H2 in water
            [10.0, 11.0, 12.0, /**/ 0.0, 0.0, 0.0, /**/ 0.0, 0.0, 0.0],
        ]);

        let gradients = descriptor.gradients.as_ref().unwrap();
        assert_eq!(*gradients, array![
            /*    H-H                  missing               O-H       */
            // O in water, derivatives w.r.t. O
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,   1.0, 0.1, -1.0],
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,   2.0, 0.2, -2.0],
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,   3.0, 0.3, -3.0],
            // O in water, derivatives w.r.t. H1
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,   4.0, 0.4, -4.0],
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,   5.0, 0.5, -5.0],
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,   6.0, 0.6, -6.0],
            // O in water, derivatives w.r.t. H2
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,   7.0, 0.7, -7.0],
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,   8.0, 0.8, -8.0],
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,   9.0, 0.9, -9.0],
            // H1 in water, derivatives w.r.t. H1
            [10.0, 0.10, -10.0,    0.0, 0.0, 0.0,   0.0, 0.0, 0.0],
            [11.0, 0.11, -11.0,    0.0, 0.0, 0.0,   0.0, 0.0, 0.0],
            [12.0, 0.12, -12.0,    0.0, 0.0, 0.0,   0.0, 0.0, 0.0],
            // H1 in water, derivatives w.r.t. H2
            [13.0, 0.13, -13.0,    0.0, 0.0, 0.0,   0.0, 0.0, 0.0],
            [14.0, 0.14, -14.0,    0.0, 0.0, 0.0,   0.0, 0.0, 0.0],
            [15.0, 0.15, -15.0,    0.0, 0.0, 0.0,   0.0, 0.0, 0.0],
            // H1 in water, derivatives w.r.t. O
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,   0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,   0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,   0.0, 0.0, 0.0],
            // H2 in water, derivatives w.r.t. H2
            [22.0, 0.22, -22.0,    0.0, 0.0, 0.0,   0.0, 0.0, 0.0],
            [23.0, 0.23, -23.0,    0.0, 0.0, 0.0,   0.0, 0.0, 0.0],
            [24.0, 0.24, -24.0,    0.0, 0.0, 0.0,   0.0, 0.0, 0.0],
            // H2 in water, derivatives w.r.t. H1
            [25.0, 0.25, -25.0,    0.0, 0.0, 0.0,   0.0, 0.0, 0.0],
            [26.0, 0.26, -26.0,    0.0, 0.0, 0.0,   0.0, 0.0, 0.0],
            [27.0, 0.27, -27.0,    0.0, 0.0, 0.0,   0.0, 0.0, 0.0],
            // H2 in water, derivatives w.r.t. O
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,   0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,   0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0,        0.0, 0.0, 0.0,   0.0, 0.0, 0.0],
        ]);
    }
}
