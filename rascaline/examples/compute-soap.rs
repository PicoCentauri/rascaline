use rascaline::{Calculator, Descriptor, System, descriptor::DotOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // load the systems from command line argument
    let path = std::env::args().nth(1).expect("expected a command line argument");
    let systems = rascaline::systems::read_from_file(path)?;
    // transform systems into a vector of trait objects (`Vec<Box<dyn System>>`)
    let mut systems = systems.into_iter()
        .map(|s| Box::new(s) as Box<dyn System>)
        .collect::<Vec<_>>();

    // pass hyper-parameters as JSON
    let parameters = r#"{
        "cutoff": 5.0,
        "max_radial": 6,
        "max_angular": 4,
        "atomic_gaussian_width": 0.3,
        "gradients": true,
        "radial_basis": {
            "Gto": {}
        },
        "cutoff_function": {
            "ShiftedCosine": {"width": 0.5}
        }
    }"#;
    // create the calculator with its name and parameters
    let mut calculator = Calculator::new("soap_power_spectrum", parameters.to_owned())?;

    // create an empty descriptor
    let mut descriptor = Descriptor::new();

    // run the calculation using default options
    calculator.compute(&mut systems[..10], &mut descriptor, Default::default())?;

    // Transform the descriptor to dense representation,
    // with one sample for each atom-centered environment.
    // descriptor.densify(&["species_neighbor_1", "species_neighbor_2"], None)?;

    // you can now use descriptor.values as the
    // input of a machine learning algorithm

    // create an empty descriptor
    let mut descriptor_sparse = Descriptor::new();

    // run the calculation using default options
    calculator.compute(&mut systems[10..12], &mut descriptor_sparse, Default::default())?;

    // Transform the descriptor to dense representation,
    // with one sample for each atom-centered environment.
    // descriptor.densify(&["species_neighbor_1", "species_neighbor_2"], None)?;

    let kernel = descriptor.dot(&descriptor_sparse, DotOptions {
        reduce_across: &["species_neighbor_1", "species_neighbor_2"],
        gradients: true,
        normalize: true
    });

    Ok(())
}
