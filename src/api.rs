use std::collections::HashMap;
use std::fs::read_dir;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use actix_web::{get, post, web, HttpResponse, Responder};

use serde::Deserialize;
// use serde_json::de::Read;
use serde_json::{json, Value};

use log;

#[derive(Clone)]
pub struct AppMutState {
    pub mappings: Arc<Mutex<HashMap<String, serde_json::Value>>>,
    pub registry_path: String,
}

#[get("/health")]
pub async fn health() -> impl Responder {
    log::info!("health");
    HttpResponse::Ok()
}

/// Funcion that reads the files in registry_path and updates the mappings
/// This function should add better error handling by returning a result so
/// the views can act accordingly
pub fn read_mappings(
    registry_path: String,
    mappings: Arc<Mutex<HashMap<String, serde_json::Value>>>,
) {
    // Can we create a PathBuf from registry_path?
    let _path_buf = PathBuf::from_str(&registry_path);
    if _path_buf.is_err() {
        log::error!("Not a string {}", &registry_path);
        return;
    }
    //Can we create a ReadDir iterator of the PathBug?
    let path_buf = _path_buf.unwrap();
    let paths = read_dir(path_buf);

    if let Ok(mut mtx) = mappings.lock() {
        // We know paths is not an error
        for path in paths.expect("Not a ReadDir Iterator") {
            let path = path.expect("Not a DirEntry").path();
            let stem_path = path.file_stem().expect("No file_stem");
            let stem_str = stem_path.to_str().expect("Failed creating str");
            // The key for hashmap is the name of the json file on disk
            let key = stem_str.to_string();
            if let Ok(raw_json) = std::fs::read_to_string(&path) {
                if let Ok(json_data) = serde_json::from_str(&raw_json) {
                    mtx.insert(key, json_data);
                }
            }
        }
        log::info!("Read {} items", mtx.len());
    } else {
        log::error!("Could not acquire mutex lock in read_mappings");
    }
}

/// Endpoint to retrieve a single subject
#[get("/metadata/{subject}")]
pub async fn single_subject(
    path: web::Path<String>,
    app_data: web::Data<AppMutState>,
) -> impl Responder {
    // aquire lock or die tryin
    match app_data.mappings.lock() {
        Ok(mtx) => {
            let subject = path.into_inner();
            match mtx.get(&subject) {
                Some(d) => {
                    return HttpResponse::Ok().json(d);
                }
                None => {
                    log::debug!("Nothing found for {}", subject);
                    return HttpResponse::NotFound().body("");
                }
            };
        },
        Err(e) => {
            log::warn!("Error acquiring mutex lock! {}", e.to_string());
            return HttpResponse::InternalServerError().body("");
        }
    }
}

/// Endpoint to retrieve all porperty names for a given subject
/// While the CIP says it should return a list of strings that are the properties
/// of given subject, the currently live implementation does not do that.
/// So this view will do the same (as does the implementation i am trying to replace)
#[get("/metadata/{subject}/properties")]
pub async fn all_properties(
    path: web::Path<String>,
    app_data: web::Data<AppMutState>,
) -> impl Responder {
    // aquire lock or die tryin
    match app_data.mappings.lock() {
        Ok(mtx) => {
            let subject = path.into_inner();
            match mtx.get(&subject) {
                Some(d) => {
                    log::debug!("Found Value for {}", subject);
                    return HttpResponse::Ok().json(d);
                }
                None => {
                    log::debug!("No Value found for {}", subject);
                    return HttpResponse::NotFound().body("");
                }
            }
        }
        Err(e) => {
            log::warn!("Error acquiring mutex lock! {}", e.to_string());
            return HttpResponse::InternalServerError().body("");
        }
    }
}

/// Endpoint to retrieve a specific property value for a given subject
/// The CIP Recommended /metadata/SUBJECT/property/NAME
/// But both other impementations have chosen to pick /metadata/SUBJECT/properties/NAME
/// https://tokens.cardano.org/metadata/5c4f08f47124b8e7ce9a4d0a00a5939da624cf6e533e1dc9de9b49c5556e636c6542656e6e793630/properties/logo
///
#[get("/metadata/{subject}/properties/{name}")]
pub async fn some_property(
    path: web::Path<(String, String)>,
    app_data: web::Data<AppMutState>,
) -> impl Responder {
    match app_data.mappings.lock() {
        Ok(mtx) => {
            let (subject, name) = path.into_inner();
            let meta = mtx.get(&subject).expect("Could not find it ");

            if let Some(v) = meta.get(name) {
                let val = json!({ "subject": &subject, "name": v });
                return HttpResponse::Ok().json(val);
            }
            return HttpResponse::NotFound().body("");
        },
        Err(e) => {
            log::warn!("Error acquiring mutex lock! {}", e.to_string());
            return HttpResponse::InternalServerError().body("");
        }
    }
}

/// Endpoint to trigger update of the data
#[get("/reread")]
pub async fn reread_mappings(app_data: web::Data<AppMutState>) -> impl Responder {
    read_mappings(app_data.registry_path.clone(), app_data.mappings.clone());
    HttpResponse::Ok().body("Reread contents")
}

/// A query payload for the batch query endpoint
#[derive(Deserialize)]
pub struct Query{
    subjects: Vec<String>,
    properties: Option<Vec<String>>,
}

/// Endpoint for batch requesting multiple subjects at once
/// If the payload holds 'properties' the subject should be narrowed down
/// to only these properties
#[post("/metadata/query")]
pub async fn query(
    payload: web::Json<Query>,
    app_data: web::Data<AppMutState>,
) -> impl Responder {
    // subjects holds subjects that where requests and should be returned
    let mut subjects: Vec<Value> = Vec::new();
    let mtx = app_data
        .mappings
        .lock()
        .expect("Error acquiring mutex lock");
    // Grab ref to properties so we can use it throughout the for loop below
    let properties = payload.properties.clone();
    log::debug!("Requested {} subjects", payload.subjects.len());
    if properties.is_some() {
        // as_ref() temporary references properties, so its not actually moved
        //   It needs to be used a little lower in the code
        log::debug!("   with {} properties", properties.as_ref().unwrap().len());
    }

    for subject in payload.subjects.iter() {
        // Find subject in mappings or do nothing
        match mtx.get(subject) {
            Some(subj) => {
                // If there are properties given in the payload, only return
                // these for each subject, if not return the whole subject
                match &properties {
                    Some(props) => {
                        // Build a new subject only with given properties
                        let mut newsubj: HashMap<&str, &Value> = HashMap::new();
                        for p in props.iter() {
                            let value = subj.get(p);
                            if value.is_some() {
                                newsubj.insert(p, value.unwrap());
                            }
                        }
                        subjects.push(serde_json::json!(newsubj));
                    },
                    None => {
                        // There are no properties given, return whole subject
                        subjects.push(subj.clone());
                    }
                }
            },
            None => {
                log::debug!("Subject not found {}", subject);
            }
        }
    }

    // let mut subjects: Vec<serde_json::Value> = Vec::new();
    //for subject in subjects.iter() {
    //    let meta = data.metadata.get(subject).expect("Could not find it ");
    //    subjects.push(meta.to_owned())
    //}
    let out = serde_json::json!({
        "subjects": subjects
    });
    HttpResponse::Ok().json(out)
}
