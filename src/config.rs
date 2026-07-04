use dotenvy::dotenv;
use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub allowed_groups: Vec<String>,
    pub admin_numbers: Vec<String>,
}

impl Config {
    pub fn load() -> Self {
        // Load .env file, ignoring errors if it doesn't exist (e.g. in prod with env vars set directly)
        let _ = dotenv();
        
        let allowed_groups = env::var("ALLOWED_GROUPS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
            
        let admin_numbers = env::var("ADMIN_NUMBERS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
            
        Self {
            allowed_groups,
            admin_numbers,
        }
    }
    
    pub fn is_group_allowed(&self, jid: &str) -> bool {
        self.allowed_groups.contains(&jid.to_string())
    }
    
    pub fn is_admin(&self, jid: &str) -> bool {
        self.admin_numbers.contains(&jid.to_string())
    }
}
