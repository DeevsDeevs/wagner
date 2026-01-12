use super::chains::ChainsState;

#[derive(Debug, Default)]
pub struct PluginStates {
    pub chains: ChainsState,
}

impl PluginStates {
    pub fn new() -> Self {
        Self::default()
    }
}
