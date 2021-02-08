use std::os::raw::c_void;

use rascaline::types::{Vector3D, Matrix3};
use rascaline::system::{System, Pair, UnitCell};

/// Pair of atoms coming from a neighbor list
#[repr(C)]
pub struct rascal_pair_t {
    /// index of the first atom in the pair
    pub first: usize,
    /// index of the second atom in the pair
    pub second: usize,
    /// vector from the first atom to the second atom, wrapped inside the unit
    /// cell as required
    pub vector: [f64; 3],
}

#[repr(C)]
pub struct rascal_system_t {
    /// User-provided data should be stored here, it will be passed as the
    /// first parameter to all function pointers
    user_data: *mut c_void,
    size: Option<unsafe extern fn(user_data: *const c_void, size: *mut usize)>,
    species: Option<unsafe extern fn(user_data: *const c_void, species: *mut *const usize)>,
    positions: Option<unsafe extern fn(user_data: *const c_void, positions: *mut *const f64)>,
    cell: Option<unsafe extern fn(user_data: *const c_void, cell: *mut f64)>,
    compute_neighbors: Option<unsafe extern fn(user_data: *mut c_void, cutoff: f64)>,
    pairs: Option<unsafe extern fn(user_data: *const c_void, pairs: *mut *const rascal_pair_t, count: *mut usize)>,
}

impl System for rascal_system_t {
    fn size(&self) -> usize {
        let mut value = 0;
        let function = self.size.expect("rascal_system_t.size is NULL");
        unsafe {
            function(self.user_data, &mut value);
        }
        return value;
    }

    fn species(&self) -> &[usize] {
        let mut ptr = std::ptr::null();
        let function = self.species.expect("rascal_system_t.species is NULL");
        unsafe {
            function(self.user_data, &mut ptr);
            // TODO: check if ptr.is_null() and error in some way?
            return std::slice::from_raw_parts(ptr, self.size());
        }
    }

    fn positions(&self) -> &[Vector3D] {
        let mut ptr = std::ptr::null();
        let function = self.positions.expect("rascal_system_t.positions is NULL");
        unsafe {
            function(self.user_data, &mut ptr);
            let slice = std::slice::from_raw_parts(ptr as *const [f64; 3], self.size());
            return &*(slice as *const [[f64; 3]] as *const [Vector3D]);
        }
    }

    fn cell(&self) -> UnitCell {
        let mut value = [[0.0; 3]; 3];
        let function = self.cell.expect("rascal_system_t.cell is NULL");
        let matrix: Matrix3 = unsafe {
            function(self.user_data, &mut value[0][0]);
            std::mem::transmute(value)
        };

        if matrix == Matrix3::zero() {
            return UnitCell::infinite();
        } else {
            return UnitCell::from(matrix);
        }
    }

    fn compute_neighbors(&mut self, cutoff: f64) {
        let function = self.compute_neighbors.expect("rascal_system_t.compute_neighbors is NULL");
        unsafe {
            function(self.user_data, cutoff);
        }
    }

    fn pairs(&self) -> &[Pair] {
        let function = self.pairs.expect("rascal_system_t.pairs is NULL");
        let mut ptr = std::ptr::null();
        let mut count = 0;
        unsafe {
            function(self.user_data, &mut ptr, &mut count);
            return std::slice::from_raw_parts(ptr as *const Pair, count);
        }
    }
}