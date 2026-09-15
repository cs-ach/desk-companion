use std::collections::HashMap;
use anyhow::{Result};

use plugin::Plugin;

pub struct Renderer {
    plugin: Plugin,
}

impl Renderer {

    pub fn new(plugin: Plugin) -> Result<Self> {
        Ok(
            Self { plugin }
        )
    }

    pub fn render(screen: String, payload: HashMap<String, String>) -> Result<()> {
        Ok(())
    }

    fn get_screen_path(&self, screen: String) {

    }

}