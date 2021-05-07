use ndarray::Array3;

use crate::{Matrix3, Vector3D};
use super::UnitCell;

/// `f64::clamp` backported to rust 1.45
fn f64_clamp(mut x: f64, min: f64, max: f64) -> f64 {
    debug_assert!(min <= max);
    if x < min {
        x = min;
    }
    if x > max {
        x = max;
    }
    return x;
}

/// `usize::clamp` backported to rust 1.45
fn usize_clamp(mut x: usize, min: usize, max: usize) -> usize {
    debug_assert!(min <= max);
    if x < min {
        x = min;
    }
    if x > max {
        x = max;
    }
    return x;
}

/// Maximal number of cells, we need to use this to prevent having too many
/// cells with a small unit cell and a large cutoff
const MAX_NUMBER_OF_CELLS: f64 = 1e5;

/// A cell shift represents the displacement along cell axis between an atom and
/// a periodic image. The cell shift can be used to reconstruct the vector
/// between two points, wrapped inside the unit cell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CellShift([isize; 3]);

impl std::ops::Add<CellShift> for CellShift {
    type Output = CellShift;

    fn add(mut self, rhs: CellShift) -> Self::Output {
        self.0[0] += rhs[0];
        self.0[1] += rhs[1];
        self.0[2] += rhs[2];
        return self;
    }
}

impl std::ops::Sub<CellShift> for CellShift {
    type Output = CellShift;

    fn sub(mut self, rhs: CellShift) -> Self::Output {
        self.0[0] -= rhs[0];
        self.0[1] -= rhs[1];
        self.0[2] -= rhs[2];
        return self;
    }
}

impl std::ops::Index<usize> for CellShift {
    type Output = isize;

    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

impl CellShift {
    pub fn dot(&self, cell: &Matrix3) -> Vector3D {
        let x = cell[0][0] * self[0] as f64 + cell[1][0] * self[1] as f64 + cell[2][0] * self[2] as f64;
        let y = cell[0][1] * self[0] as f64 + cell[1][1] * self[1] as f64 + cell[2][1] * self[2] as f64;
        let z = cell[0][2] * self[0] as f64 + cell[1][2] * self[1] as f64 + cell[2][2] * self[2] as f64;
        Vector3D::new(x, y, z)
    }
}

/// Pair produced by the cell list. The vector between the atoms can be
/// constructed as `position[second] - position[first] + shift.dot(unit_cell)`
#[derive(Debug, Clone)]
pub struct CellPair {
    /// index of the first atom in the pair
    pub first: usize,
    /// index of the second atom in the pair
    pub second: usize,
    /// number of shifts along the cell for this pair
    pub shift: CellShift,
}

#[derive(Debug, Clone)]
pub struct CellList {
    /// How many cells do we need to look at when searching neighbors to include
    /// all neighbors below cutoff
    n_search: [isize; 3],
    /// the cells themselves are represented as an array of atom indexes within
    /// this cell, together with the shift vector from the actual atom position
    /// to a position inside the unit cell
    cells: ndarray::Array3<Vec<(usize, CellShift)>>,
    /// Unit cell defining periodic boundary conditions
    unit_cell: UnitCell,
}

impl CellList {
    /// Create a new `CellList` for the given unit cell and cutoff, determining
    /// all required parameters.
    pub fn new(unit_cell: UnitCell, cutoff: f64) -> CellList {
        let distances_between_faces = if unit_cell.is_infinite() {
            // use a pseudo orthorhombic cell with size cutoff
            Vector3D::new(1.0, 1.0, 1.0)
        } else {
            unit_cell.distances_between_faces()
        };

        let mut n_cells = [
            f64_clamp(f64::trunc(distances_between_faces[0] / cutoff), 1.0, f64::INFINITY),
            f64_clamp(f64::trunc(distances_between_faces[1] / cutoff), 1.0, f64::INFINITY),
            f64_clamp(f64::trunc(distances_between_faces[2] / cutoff), 1.0, f64::INFINITY),
        ];

        assert!(n_cells[0].is_finite() && n_cells[1].is_finite() && n_cells[2].is_finite());

        // limit memory consumption by ensuring we have less than `MAX_N_CELLS`
        // cells to look though
        let n_cells_total = n_cells[0] * n_cells[1] * n_cells[2];
        if n_cells_total > MAX_NUMBER_OF_CELLS {
            // set the total number of cells close to MAX_N_CELLS, while keeping
            // roughly the ratio of cells in each direction
            let ratio_x_y = n_cells[0] / n_cells[1];
            let ratio_y_z = n_cells[1] / n_cells[2];

            n_cells[2] = f64::trunc(f64::cbrt(MAX_NUMBER_OF_CELLS / (ratio_x_y * ratio_y_z * ratio_y_z)));
            n_cells[1] = f64::trunc(ratio_y_z * n_cells[2]);
            n_cells[0] = f64::trunc(ratio_x_y * n_cells[1]);
        }

        // number of cells to search in each direction to make sure all possible
        // pairs below the cutoff are accounted for.
        let mut n_search = [
            f64::trunc(cutoff * n_cells[0] / distances_between_faces[0]) as isize,
            f64::trunc(cutoff * n_cells[1] / distances_between_faces[1]) as isize,
            f64::trunc(cutoff * n_cells[2] / distances_between_faces[2]) as isize,
        ];

        let n_cells = [
            n_cells[0] as usize,
            n_cells[1] as usize,
            n_cells[2] as usize,
        ];

        for spatial in 0..3 {
            if n_search[spatial] < 1 {
                n_search[spatial] = 1;
            }

            // don't look for neighboring cells if we have only one cell and no
            // periodic boundary condition
            if n_cells[spatial] == 1 && unit_cell.is_infinite() {
                n_search[spatial] = 0;
            }
        }

        CellList {
            n_search: n_search,
            cells: Array3::from_elem(n_cells, Default::default()),
            unit_cell: unit_cell,
        }
    }

    /// Add a single atom to the cell list at the given `position`. The atom is
    /// uniquely identified by its `index`.
    pub fn add_atom(&mut self, index: usize, position: Vector3D) {
        let fractional = if self.unit_cell.is_infinite() {
            position
        } else {
            self.unit_cell.fractional(position)
        };

        let n_cells = self.cells.shape();
        let n_cells = [n_cells[0], n_cells[1], n_cells[2]];

        // find the cell in which this atom should go
        let cell_index = [
            f64::floor(fractional[0] * n_cells[0] as f64) as isize,
            f64::floor(fractional[1] * n_cells[1] as f64) as isize,
            f64::floor(fractional[2] * n_cells[2] as f64) as isize,
        ];

        // deal with pbc by wrapping the atom inside if it was outside of the
        // cell
        let (shift, cell_index) = if self.unit_cell.is_infinite() {
            let cell_index = [
                usize_clamp(cell_index[0] as usize, 0, n_cells[0] - 1),
                usize_clamp(cell_index[1] as usize, 0, n_cells[1] - 1),
                usize_clamp(cell_index[2] as usize, 0, n_cells[2] - 1),
            ];
            ([0, 0, 0], cell_index)
        } else {
            divmod_vec(cell_index, n_cells)
        };

        self.cells[cell_index].push((index, CellShift(shift)));
    }

    pub fn pairs(&self) -> Vec<CellPair> {
        let mut pairs = Vec::new();

        let n_cells = self.cells.shape();
        let n_cells = [n_cells[0], n_cells[1], n_cells[2]];

        let search_x = -self.n_search[0]..=self.n_search[0];
        let search_y = -self.n_search[1]..=self.n_search[1];
        let search_z = -self.n_search[2]..=self.n_search[2];

        // for each cell in the cell list
        for ((cell_i_x, cell_i_y, cell_i_z), current_cell) in self.cells.indexed_iter() {
            // look through each neighboring cell
            for delta_x in search_x.clone() {
                for delta_y in search_y.clone() {
                    for delta_z in search_z.clone() {
                        let cell_i = [
                            cell_i_x as isize + delta_x,
                            cell_i_y as isize + delta_y,
                            cell_i_z as isize + delta_z,
                        ];

                        // shift vector from one cell to the other and index of
                        // the neighboring cell
                        let (cell_shift, neighbor_cell_i) = divmod_vec(cell_i, n_cells);

                        for &(atom_i, shift_i) in current_cell {
                            for &(atom_j, shift_j) in &self.cells[neighbor_cell_i] {
                                // create a half neighbor list
                                if atom_i > atom_j {
                                    continue;
                                }

                                let shift = CellShift(cell_shift) + shift_i - shift_j;

                                if atom_i == atom_j && (shift[0] == 0 && shift[1] == 0 && shift[2] == 0) {
                                    // only create pair with the same atom twice
                                    // if the pair spans more than one unit cell
                                    continue;
                                }

                                if self.unit_cell.is_infinite() && (shift[0] != 0 || shift[1] != 0 || shift[2] != 0) {
                                    // do not create pairs crossing the periodic
                                    // boundaries in an infinite cell
                                    continue;
                                }

                                pairs.push(CellPair {
                                    first: atom_i,
                                    second: atom_j,
                                    shift: shift,
                                });
                            }
                        } // loop over atoms in current neighbor cells

                    }
                }
            } // loop over neighboring cells

        }

        return pairs;
    }
}


/// Function to compute both quotient and remainder of the division of a by b.
/// This function follows Python convention, making sure the remainder have the
/// same sign as `b`.
fn divmod(a: isize, b: usize) -> (isize, usize) {
    let b = b as isize;
    let mut quotient = a / b;
    let mut remainder = a % b;
    if remainder < 0 {
        remainder += b;
        quotient -= 1;
    }
    return (quotient, remainder as usize);
}

/// Apply the [`divmod`] function to three components at the time
fn divmod_vec(a: [isize; 3], b: [usize; 3]) -> ([isize; 3], [usize; 3]) {
    let (qx, rx) = divmod(a[0], b[0]);
    let (qy, ry) = divmod(a[1], b[1]);
    let (qz, rz) = divmod(a[2], b[2]);
    return ([qx, qy, qz], [rx, ry, rz]);
}