use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub enum AtmosphereElement {
    Water,
    Oxygen,
    CarbonDioxide,
    SulphurDioxide,
    Ammonia,
    Methane,
    Nitrogen,
    Hydrogen,
    Helium,
    Neon,
    Argon,
    Silicates,
    Iron,
}
