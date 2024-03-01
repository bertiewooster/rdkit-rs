use std::{
    collections::HashMap,
    ffi::{c_int, c_uint, CStr, CString},
    fmt::Display,
};

use bitflags::bitflags;
use rdkit_sys::{
    RDKit_MolToSmiles, RDKit_ROMol, RDKit_ROMol_delete, RDKit_SDMolSupplier,
    RDKit_SmartsToMol, RDKit_SmilesToMol, RDKit_create_mol_supplier,
    RDKit_delete_mol_supplier, RDKit_mol_supplier_at_end,
    RDKit_mol_supplier_next,
};

use self::bitvector::BitVector;

pub mod fragment;

pub struct SDMolSupplier(*mut RDKit_SDMolSupplier);

unsafe impl Send for SDMolSupplier {}

impl SDMolSupplier {
    /// construct an [SDMolSupplier] from a filepath that can be converted to a
    /// CString. panics if this conversion fails.
    pub fn new(path: impl Into<Vec<u8>>) -> Result<Self, RDError> {
        let cpath = CString::new(path).expect("failed to create CString");
        unsafe {
            let inner = RDKit_create_mol_supplier(cpath.as_ptr(), true);
            if inner.is_null() {
                return Err(RDError);
            }
            Ok(Self(inner))
        }
    }

    /// reports whether or not `self` is at the end of the underlying file
    pub fn at_end(&self) -> bool {
        unsafe { RDKit_mol_supplier_at_end(self.0) }
    }
}

impl Drop for SDMolSupplier {
    fn drop(&mut self) {
        unsafe {
            RDKit_delete_mol_supplier(self.0);
        }
    }
}

#[derive(Debug)]
pub struct RDError;

impl Display for RDError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RDError: see stderr for exception info")
    }
}

impl std::error::Error for RDError {}

impl Iterator for SDMolSupplier {
    type Item = Result<ROMol, RDError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.at_end() {
            return None;
        }
        unsafe {
            let mol = RDKit_mol_supplier_next(self.0);
            if mol.is_null() {
                return Some(Err(RDError));
            }
            Some(Ok(ROMol(mol)))
        }
    }
}

/// WARNING: use this at your own risk. I don't think I did anything wrong in
/// the C++ wrapper, and it's using the default of 2 writer threads, but I've
/// gotten segfaults reading the ChEMBL 33 SDF. RDKit should have been built
/// with the default RDK_BUILD_THREADSAFE_SSS=ON, so defining that macro in the
/// wrapper should be safe
pub struct MultithreadedSDMolSupplier(
    *mut rdkit_sys::RDKit_MultithreadedSDMolSupplier,
);

unsafe impl Send for MultithreadedSDMolSupplier {}

impl MultithreadedSDMolSupplier {
    /// construct an [MultithreadedSDMolSupplier] from a filepath that can be
    /// converted to a CString. panics if this conversion fails
    pub fn new(path: impl Into<Vec<u8>>) -> Self {
        let cpath = CString::new(path).expect("failed to create CString");
        unsafe {
            let inner =
                rdkit_sys::RDKit_MultithreadedSDMolSupplier_new(cpath.as_ptr());
            Self(inner)
        }
    }

    /// reports whether or not `self` is at the end of the underlying file
    pub fn at_end(&self) -> bool {
        unsafe { rdkit_sys::RDKit_MultithreadedSDMolSupplier_at_end(self.0) }
    }
}

impl Drop for MultithreadedSDMolSupplier {
    fn drop(&mut self) {
        unsafe {
            rdkit_sys::RDKit_MultithreadedSDMolSupplier_delete(self.0);
        }
    }
}

impl Iterator for MultithreadedSDMolSupplier {
    type Item = ROMol;

    fn next(&mut self) -> Option<Self::Item> {
        if self.at_end() {
            return None;
        }
        Some(unsafe {
            ROMol(rdkit_sys::RDKit_MultithreadedSDMolSupplier_next(self.0))
        })
    }
}

pub mod bitvector {
    use std::ops::Index;

    pub struct BitVector {
        data: Vec<u64>,
    }

    impl BitVector {
        pub fn new() -> Self {
            Self { data: Vec::new() }
        }

        pub fn iter(&self) -> impl Iterator<Item = &u64> {
            self.data.iter()
        }

        pub fn len(&self) -> usize {
            self.data.len()
        }

        #[must_use]
        pub fn is_empty(&self) -> bool {
            self.len() == 0
        }

        pub fn count(&self) -> usize {
            self.data
                .iter()
                .fold(0, |acc, n| acc + n.count_ones() as usize)
        }
    }

    impl Default for BitVector {
        fn default() -> Self {
            Self::new()
        }
    }

    impl From<&[bool]> for BitVector {
        fn from(value: &[bool]) -> Self {
            let mut bv = BitVector::new();
            for (i, v) in value.iter().enumerate() {
                if bv.data.len() < i / 64 + 1 {
                    bv.data.push(0);
                }
                bv.data[i / 64] |= (*v as u64) << (i % 64);
            }
            bv
        }
    }

    impl Index<usize> for BitVector {
        type Output = u64;

        fn index(&self, index: usize) -> &Self::Output {
            &self.data[index]
        }
    }

    impl std::fmt::Debug for BitVector {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            for row in &self.data {
                writeln!(f, "{:064b}", row)?;
            }
            Ok(())
        }
    }
}

pub struct ROMol(*mut RDKit_ROMol);

impl Display for ROMol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_smiles())
    }
}

impl ROMol {
    pub fn from_smiles(smiles: &str) -> Self {
        let s = CString::new(smiles).expect("failed to create CString");
        unsafe { Self(RDKit_SmilesToMol(s.as_ptr())) }
    }

    pub fn from_smarts(smarts: &str) -> Self {
        let s = CString::new(smarts).expect("failed to create CString");
        unsafe { Self(RDKit_SmartsToMol(s.as_ptr())) }
    }

    pub fn to_smiles(&self) -> String {
        unsafe {
            let smiles = RDKit_MolToSmiles(self.0);
            let s = CStr::from_ptr(smiles);
            s.to_str().unwrap().to_owned()
        }
    }

    pub fn to_inchi_key(&self) -> String {
        unsafe {
            let smiles = rdkit_sys::RDKit_MolToInchiKey(self.0);
            let s = CStr::from_ptr(smiles);
            s.to_str().unwrap().to_owned()
        }
    }

    pub fn num_atoms(&self) -> usize {
        unsafe { rdkit_sys::RDKit_ROMol_getNumAtoms(self.0) as usize }
    }

    pub fn sanitize(&mut self, ops: SanitizeFlags) {
        unsafe {
            let status = rdkit_sys::RDKit_SanitizeMol(self.0, ops.bits());
            if status != 0 {
                panic!("sanitization failed");
            }
        }
    }

    pub fn set_aromaticity(&mut self, mdl: AromaticityModel) {
        unsafe {
            rdkit_sys::RDKit_SetAromaticity(self.0, mdl.bits());
        }
    }

    pub fn assign_stereochemistry(&mut self) {
        unsafe {
            rdkit_sys::RDKit_AssignStereochemistry(self.0);
        }
    }

    pub fn add_hs(&mut self) {
        unsafe {
            rdkit_sys::RDKit_AddHs(self.0);
        }
    }

    /// performs the sequence of cleaning operations used by the openff-toolkit:
    /// sanitize the molecule with ALL ^ ADJUSTHS ^ SETAROMATICITY, set
    /// aromaticity to MDL, assign stereochemistry, and add hydrogens.
    pub fn openff_clean(&mut self) {
        self.sanitize(
            SanitizeFlags::ALL
                ^ SanitizeFlags::ADJUSTHS
                ^ SanitizeFlags::SETAROMATICITY,
        );
        self.set_aromaticity(AromaticityModel::MDL);
        self.assign_stereochemistry();
        self.add_hs();
    }

    pub fn morgan_fingerprint(&self, radius: c_uint) -> HashMap<usize, usize> {
        unsafe {
            let mut len = 0;
            let ptr =
                rdkit_sys::RDKit_MorganFingerprint(self.0, radius, &mut len);
            let v1 = Vec::from_raw_parts(ptr, len, len);
            v1.into_iter()
                .map(|v| (v.bit as usize, v.count as usize))
                .collect()
        }
    }

    pub fn morgan_fingerprint_bit_vec<const N: usize>(
        &self,
        radius: c_uint,
    ) -> BitVector {
        unsafe {
            let mut tmp = [false; N];
            rdkit_sys::RDKit_MorganFingerprintBitVector(
                self.0,
                radius,
                N,
                tmp.as_mut_ptr(),
            );
            tmp.as_ref().into()
        }
    }

    pub fn draw_svg(
        &self,
        width: usize,
        height: usize,
        legend: &str,
        highlight_atoms: &[usize],
    ) -> String {
        let atoms: Vec<_> =
            highlight_atoms.iter().map(|a| *a as c_int).collect();
        unsafe {
            let legend = CString::new(legend).unwrap();
            let s = rdkit_sys::RDKit_MolDrawSVG(
                self.0,
                width as c_int,
                height as c_int,
                legend.as_ptr(),
                atoms.as_ptr(),
                highlight_atoms.len(),
            );
            CString::from_raw(s).to_str().unwrap().to_owned()
        }
    }

    pub fn replace_substructs(
        &self,
        query: &ROMol,
        replacement: &ROMol,
        all: bool,
    ) -> Vec<ROMol> {
        unsafe {
            let mut len = 0;
            let res = rdkit_sys::RDKit_ReplaceSubstructs(
                self.0,
                query.0,
                replacement.0,
                all,
                &mut len,
            );
            assert!(!res.is_null());
            let mut ret = Vec::with_capacity(len);
            for i in 0..len {
                ret.push(ROMol(*res.add(i)));
            }
            ret
        }
    }
}

impl Clone for ROMol {
    fn clone(&self) -> Self {
        unsafe { Self(rdkit_sys::RDKit_ROMol_copy(self.0)) }
    }
}

impl Drop for ROMol {
    fn drop(&mut self) {
        unsafe {
            RDKit_ROMol_delete(self.0);
        }
    }
}

unsafe impl Send for ROMol {}
unsafe impl Sync for ROMol {}

bitflags! {
    pub struct SanitizeFlags: c_uint {
        const NONE =                    0x0;
        const CLEANUP =                 0x1;
        const PROPERTIES =              0x2;
        const SYMMRINGS =               0x4;
        const KEKULIZE =                0x8;
        const FINDRADICALS =            0x10;
        const SETAROMATICITY =          0x20;
        const SETCONJUGATION =          0x40;
        const SETHYBRIDIZATION =        0x80;
        const CLEANUPCHIRALITY =        0x100;
        const ADJUSTHS =                0x200;
        const CLEANUP_ORGANOMETALLICS = 0x400;
        const ALL =                     0xFFFFFFF;
    }
}

bitflags! {
    pub struct AromaticityModel: c_uint {
        const DEFAULT = 0x0;
        const RDKIT = 0x1;
        const SIMPLE = 0x2;
        const MDL = 0x4;
        const CUSTOM = 0xFFFFFFF;
    }
}

pub fn find_smarts_matches(mol: &ROMol, smarts: &str) -> Vec<Vec<usize>> {
    let mut len = 0;
    let mut match_size = 0;
    let smarts = CString::new(smarts).unwrap();
    unsafe {
        let matches = rdkit_sys::find_smarts_matches(
            mol.0,
            smarts.as_ptr(),
            &mut len,
            &mut match_size,
        );
        let matches = Vec::from_raw_parts(matches, len, len);

        let mut ret = Vec::new();
        for mat in matches.chunks(match_size) {
            ret.push(mat.iter().map(|&x| x as usize).collect());
        }
        ret
    }
}

/// returns the sequence of chemical environment "tuples" that match `smarts` in
/// `mol`.
pub fn find_smarts_matches_mol(mol: &ROMol, smarts: &ROMol) -> Vec<Vec<usize>> {
    let mut len = 0;
    let mut match_size = 0;
    unsafe {
        let matches = rdkit_sys::find_smarts_matches_mol(
            mol.0,
            smarts.0,
            &mut len,
            &mut match_size,
        );
        let matches = Vec::from_raw_parts(matches, len, len);

        let mut ret = Vec::new();
        for mat in matches.chunks(match_size) {
            ret.push(mat.iter().map(|&x| x as usize).collect());
        }
        ret
    }
}

pub mod fingerprint {
    use super::bitvector::BitVector;

    /// print `bv` to stdout in 16 groups of 4 per row
    pub fn print_bit_vec(bv: &[usize]) {
        for line in bv.chunks(16 * 4) {
            for chunk in line.chunks(4) {
                print!(" ");
                for elt in chunk {
                    print!("{elt}");
                }
            }
            println!();
        }
    }

    /// return the number of bits in the intersection of a and b
    pub fn intersect(a: &BitVector, b: &BitVector) -> usize {
        a.iter()
            .zip(b.iter())
            .fold(0, |acc, (a, b)| acc + (a & b).count_ones()) as usize
    }

    /// Computes the Tanimoto distance between bit vectors a and b
    ///
    /// T(a, b) = (a ∩ b) / (a + b - a ∩ b), at least according to
    /// featurebase.com/blog/tanimoto-and-chemical-similarity-in-featurebase
    pub fn tanimoto(a: &BitVector, b: &BitVector) -> f64 {
        let num = intersect(a, b);
        let den: usize = a.count() + b.count() - num;
        num as f64 / den as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_inchi_key() {
        let benzene = ROMol::from_smiles("C1=CC=CC=C1");
        let got = benzene.to_inchi_key();
        let want = "UHOVQNZJYSORNB-UHFFFAOYSA-N";
        assert_eq!(got, want);
    }
}