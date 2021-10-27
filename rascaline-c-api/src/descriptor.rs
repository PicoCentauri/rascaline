use std::ops::{Deref, DerefMut};
use std::os::raw::c_char;
use std::ffi::CStr;

use rascaline::descriptor::{Descriptor, IndexValue, DotOptions};
use rascaline::Error;
use super::{catch_unwind, rascal_status_t};

/// Opaque type representing a `Descriptor`.
#[allow(non_camel_case_types)]
pub struct rascal_descriptor_t(Descriptor);

impl Deref for rascal_descriptor_t {
    type Target = Descriptor;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for rascal_descriptor_t {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Create a new empty descriptor.
///
/// All memory allocated by this function can be released using
/// `rascal_descriptor_free`.
///
/// @returns A pointer to the newly allocated descriptor, or a `NULL` pointer in
///          case of error. In case of error, you can use `rascal_last_error()`
///          to get the error message.
#[no_mangle]
#[allow(clippy::module_name_repetitions)]
pub unsafe extern fn rascal_descriptor() -> *mut rascal_descriptor_t {
    let descriptor = Box::new(rascal_descriptor_t(Descriptor::new()));
    return Box::into_raw(descriptor);
}

/// Free the memory associated with a `descriptor` previously created with
/// `rascal_descriptor`.
///
/// If `descriptor` is `NULL`, this function does nothing.
///
/// @param descriptor pointer to an existing descriptor, or `NULL`
///
/// @returns The status code of this operation. If the status is not
///          `RASCAL_SUCCESS`, you can use `rascal_last_error()` to get the
///          full error message.
#[no_mangle]
pub unsafe extern fn rascal_descriptor_free(descriptor: *mut rascal_descriptor_t) -> rascal_status_t {
    catch_unwind(|| {
        if !descriptor.is_null() {
            let boxed = Box::from_raw(descriptor);
            std::mem::drop(boxed);
        }
        Ok(())
    })
}

/// Get the values stored inside this descriptor after a call to
/// `rascal_calculator_compute`.
///
/// This function sets `*data` to a pointer containing the address of first
/// element of the 2D array containing the values, `*samples` to the size of the
/// first axis of this array and `*features` to the size of the second axis of
/// the array. The array is stored using a row-major layout.
///
/// @param descriptor pointer to an existing descriptor
/// @param data pointer to a pointer to a double, will be set to the address of
///             the first element in the values array
/// @param samples pointer to a single integer, will be set to the first
///                dimension of the values array
/// @param features pointer to a single integer, will be set to the second
///                 dimension of the values array
///
/// @returns The status code of this operation. If the status is not
///          `RASCAL_SUCCESS`, you can use `rascal_last_error()` to get the full
///          error message.
#[no_mangle]
pub unsafe extern fn rascal_descriptor_values(
    descriptor: *mut rascal_descriptor_t,
    data: *mut *mut f64,
    samples: *mut usize,
    features: *mut usize
) -> rascal_status_t {
    catch_unwind(|| {
        check_pointers!(descriptor, data, samples, features);

        let array = &mut (*descriptor).values;
        if array.is_empty() {
            *data = std::ptr::null_mut();
        } else {
            *data = array.as_mut_ptr();
        }

        let shape = array.shape();
        *samples = shape[0];
        *features = shape[1];

        Ok(())
    })
}

#[allow(clippy::doc_markdown)]
/// Get the gradients stored inside this descriptor after a call to
/// `rascal_calculator_compute`, if any.
///
/// This function sets `*data` to to a pointer containing the address of the
/// first element of the 2D array containing the gradients, `*gradient_samples`
/// to the size of the first axis of this array and `*features` to the size of
/// the second axis of the array. The array is stored using a row-major layout.
///
/// If this descriptor does not contain gradient data, `*data` is set to `NULL`,
/// while `*gradient_samples` and `*features` are set to 0.
///
/// @param descriptor pointer to an existing descriptor
/// @param data pointer to a pointer to a double, will be set to the address of
///             the first element in the gradients array
/// @param gradient_samples pointer to a single integer, will be set to the first
///                         dimension of the gradients array
/// @param features pointer to a single integer, will be set to the second
///                 dimension of the gradients array
///
/// @returns The status code of this operation. If the status is not
///          `RASCAL_SUCCESS`, you can use `rascal_last_error()` to get the full
///          error message.
#[no_mangle]
pub unsafe extern fn rascal_descriptor_gradients(
    descriptor: *mut rascal_descriptor_t,
    data: *mut *mut f64,
    gradient_samples: *mut usize,
    features: *mut usize
) -> rascal_status_t {
    catch_unwind(|| {
        check_pointers!(descriptor, data, gradient_samples, features);

        if let Some(ref mut array) = (*descriptor).gradients {
            *data = array.as_mut_ptr();
            let shape = array.shape();
            *gradient_samples = shape[0];
            *features = shape[1];
        } else {
            *data = std::ptr::null_mut();
            *gradient_samples = 0;
            *features = 0;
        }

        Ok(())
    })
}

#[repr(C)]
#[allow(non_camel_case_types)]
/// The different kinds of indexes that can exist on a `rascal_descriptor_t`
pub enum rascal_indexes {
    /// The feature index, describing the features of the representation
    RASCAL_INDEXES_FEATURES = 0,
    /// The samples index, describing different samples in the representation
    RASCAL_INDEXES_SAMPLES = 1,
    /// The gradient samples index, describing the gradients of samples in the
    /// representation with respect to other atoms
    RASCAL_INDEXES_GRADIENT_SAMPLES = 2,
}

/// Get the values associated with one of the `indexes` in the given
/// `descriptor`.
///
/// This function sets `*data` to to a **read only** pointer containing the
/// address of the first element of the 2D array containing the index values,
/// `*count` to the number of indexes (first dimension of the array) and `*size`
/// to the size of each index (second dimension of the array). The array is
/// stored using a row-major layout.
///
/// If this `descriptor` does not contain gradient data, and `indexes` is
/// `RASCAL_INDEXES_GRADIENTS`, `*data` is set to `NULL`, while
/// `*count` and `*size` are set to 0.
///
/// @param descriptor pointer to an existing descriptor
/// @param indexes type of indexes requested
/// @param data pointer to a pointer to a double, will be set to the address of
///             the first element in the index array
/// @param count pointer to a single integer, will be set to the number of
///              index values
/// @param size pointer to a single integer, will be set to the size of each
///              index value
///
/// @returns The status code of this operation. If the status is not
///          `RASCAL_SUCCESS`, you can use `rascal_last_error()` to get the full
///          error message.
#[no_mangle]
pub unsafe extern fn rascal_descriptor_indexes(
    descriptor: *const rascal_descriptor_t,
    indexes: rascal_indexes,
    data: *mut *const i32,
    count: *mut usize,
    size: *mut usize,
) -> rascal_status_t {
    catch_unwind(|| {
        check_pointers!(descriptor, data, size, count);

        let indexes = match indexes {
            rascal_indexes::RASCAL_INDEXES_FEATURES => &(*descriptor).features,
            rascal_indexes::RASCAL_INDEXES_SAMPLES => &(*descriptor).samples,
            rascal_indexes::RASCAL_INDEXES_GRADIENT_SAMPLES => {
                if let Some(indexes) = &(*descriptor).gradients_samples {
                    indexes
                } else {
                    *data = std::ptr::null();
                    *size = 0;
                    *count = 0;
                    return Ok(());
                }
            }
        };

        *size = indexes.size();
        *count = indexes.count();
        if *count == 0 {
            *data = std::ptr::null();
        } else {
            *data = (&indexes[0][0] as *const IndexValue).cast();
        }

        Ok(())
    })
}

/// Get the names associated with one of the `indexes` in the given
/// `descriptor`.
///
/// If this `descriptor` does not contain gradient data, and `indexes` is
/// `RASCAL_INDEXES_GRADIENTS`, each pointer in `*names` is set to `NULL`.
///
/// The `size` value should correspond to the value set by
/// `rascal_descriptor_indexes` in the `size` parameter.
///
/// @param descriptor pointer to an existing descriptor
/// @param indexes type of indexes requested
/// @param names pointer to the first element of an array of `const char*`
///              that will be filled with **read only** pointers to the index
///              names
/// @param size size of the `names` array, i.e. number of elements inside
///             the array
///
/// @returns The status code of this operation. If the status is not
///          `RASCAL_SUCCESS`, you can use `rascal_last_error()` to get the full
///          error message.
#[allow(clippy::missing_panics_doc)]
#[no_mangle]
pub unsafe extern fn rascal_descriptor_indexes_names(
    descriptor: *const rascal_descriptor_t,
    indexes: rascal_indexes,
    names: *mut *const c_char,
    size: usize
) -> rascal_status_t {
    catch_unwind(|| {
        check_pointers!(descriptor, names);

        let indexes = match indexes {
            rascal_indexes::RASCAL_INDEXES_FEATURES => &(*descriptor).features,
            rascal_indexes::RASCAL_INDEXES_SAMPLES => &(*descriptor).samples,
            rascal_indexes::RASCAL_INDEXES_GRADIENT_SAMPLES => {
                if let Some(indexes) = &(*descriptor).gradients_samples {
                    indexes
                } else {
                    for i in 0..size {
                        names.add(i).write(std::ptr::null());
                    }
                    return Ok(());
                }
            }
        };

        if size != indexes.c_names().len() {
            return Err(Error::InvalidParameter(
                format!(
                    "not enough space for all names in these indexes: \
                    we need {} entries but the buffer only have space for {}",
                    indexes.c_names().len(), size
                )
            ));
        }

        for (i, name) in indexes.c_names().iter().enumerate() {
            assert!(i < size);
            names.add(i).write(name.as_ptr());
        }

        if size > indexes.size() {
            for i in indexes.size()..size {
                names.add(i).write(std::ptr::null());
            }
        }

        Ok(())
    })
}

/// Make the given `descriptor` dense along the given `variables`.
///
/// The `variable` array should contain the name of the variables as
/// NULL-terminated strings, and `variables_count` must be the number of
/// variables in the array.
///
/// The `requested` parameter defines which set of values taken by the
/// `variables` should be part of the new features. If it is `NULL`, this is the
/// set of values taken by the variables in the samples. Otherwise, it must be a
/// pointer to the first element of a 2D row-major array with one row for each
/// new feature block, and one column for each variable. `requested_size` must
/// be the number of rows in this array.
///
/// This function "moves" the variables from the samples to the features,
/// filling the new features with zeros if the corresponding sample is missing.
///
/// For example, take a descriptor containing two samples variables (`structure`
/// and `species`) and two features (`n` and `l`). Starting with this
/// descriptor:
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
/// Calling `descriptor.densify(["species"])` will move `species` out of the
/// samples and into the features, producing:
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
/// Notice how there is only one row/sample for each structure now, and how each
/// value for `species` have created a full block of features. Missing values
/// (e.g. structure 0/species 8) have been filled with 0.
#[no_mangle]
pub unsafe extern fn rascal_descriptor_densify(
    descriptor: *mut rascal_descriptor_t,
    variables: *const *const c_char,
    variables_count: usize,
    requested: *const i32,
    requested_size: usize,
) -> rascal_status_t {
    catch_unwind(|| {
        check_pointers!(descriptor, variables);
        let mut rust_variables = Vec::new();
        for &variable in std::slice::from_raw_parts(variables, variables_count) {
            check_pointers!(variable);
            let variable = CStr::from_ptr(variable).to_str()?;
            rust_variables.push(variable);
        }

        let requested = if requested.is_null() {
            None
        } else {
            Some(ndarray::ArrayView2::from_shape_ptr(
                [requested_size, variables_count], requested.cast::<IndexValue>()
            ))
        };

        return (*descriptor).densify(&rust_variables, requested);
    })
}

/// TODO: documentation
#[no_mangle]
pub unsafe extern fn rascal_descriptor_dot(
    lhs: *const rascal_descriptor_t,
    rhs: *const rascal_descriptor_t,
    output: *mut rascal_descriptor_t,
    reduce_across: *const *const c_char,
    reduce_across_count: usize,
    gradients: bool,
    normalize: bool,
) -> rascal_status_t {
    catch_unwind(|| {
        check_pointers!(lhs, rhs, output, reduce_across);

        let mut rust_reduce_across = Vec::new();
        for &variable in std::slice::from_raw_parts(reduce_across, reduce_across_count) {
            check_pointers!(variable);
            let variable = CStr::from_ptr(variable).to_str()?;
            rust_reduce_across.push(variable);
        }

        let options = DotOptions {
            reduce_across: &rust_reduce_across,
            gradients: gradients,
            normalize: normalize,
        };

        (*output).0 = (*lhs).dot(&*rhs, options)?;

        Ok(())
    })
}
