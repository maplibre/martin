use actix_http::Method;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CorsMode {
    Disable,
    Custom(CustomCors),
}

impl Default for CorsMode {
    fn default() -> Self {
        CorsMode::Custom(CustomCors::default())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CustomCors {
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<Method>,
    pub max_age: Option<usize>,
}

impl Default for CustomCors {
    fn default() -> Self {
        Self {
            allowed_origins: vec!["*".to_string()],
            allowed_methods: vec![Method::GET],
            max_age: None,
        }
    }
}

impl<'de> Deserialize<'de> for CustomCors {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper {
            allowed_origins: Vec<String>,
            allowed_methods: Vec<String>,
            max_age: Option<usize>,
        }

        let helper = Helper::deserialize(deserializer)?;

        let methods = helper
            .allowed_methods
            .iter()
            .map(|method| {
                Method::from_bytes(method.as_bytes())
                    .map_err(|_| serde::de::Error::custom(format!("Invalid HTTP method: {method}")))
            })
            .collect::<Result<Vec<Method>, _>>()?;

        Ok(CustomCors {
            allowed_origins: helper.allowed_origins,
            allowed_methods: methods,
            max_age: helper.max_age,
        })
    }
}

impl Serialize for CustomCors {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Helper struct to serialize methods as strings
        #[derive(Serialize)]
        struct Helper<'a> {
            allowed_origins: &'a Vec<String>,
            allowed_methods: Vec<String>,
            max_age: &'a Option<usize>,
        }

        let methods: Vec<String> = self
            .allowed_methods
            .iter()
            .map(|m| m.as_str().to_string())
            .collect();

        let helper = Helper {
            allowed_origins: &self.allowed_origins,
            allowed_methods: methods,
            max_age: &self.max_age,
        };

        helper.serialize(serializer)
    }
}

impl CorsMode {
    pub fn make_cors(&self) -> Option<actix_cors::Cors> {
        match self {
            CorsMode::Disable => None,
            CorsMode::Custom(custom) => {
                // start with the recommended restrictive library defaults
                let mut cors = actix_cors::Cors::default();

                // allow any origin by default
                // note that this will set `access-control-allow-origin` to the value of the `Origin` request header
                if custom.allowed_origins.contains(&"*".to_string()) {
                    cors = cors.allow_any_origin();
                }
                // if specific origins are provided, set them instead
                else {
                    for origin in &custom.allowed_origins {
                        cors = cors.allowed_origin(origin);
                    }
                }

                // sets `Access-Control-Allow-Methods`
                cors = cors.allowed_methods(custom.allowed_methods.clone());

                // sets `Access-Control-Max-Age`
                cors = cors.max_age(custom.max_age);

                Some(cors)
            }
        }
    }
}
