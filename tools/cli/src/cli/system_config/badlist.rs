use crate::common::OpType;
use crate::{handle_client_error, PwBadlistOpt};

// use std::thread;
use std::fs::File;
use std::io::Read;
use tokio::task;
use zxcvbn::Score;

const CHUNK_SIZE: usize = 1000;

impl PwBadlistOpt {
    pub fn debug(&self) -> bool {
        match self {
            PwBadlistOpt::Show(copt) => copt.debug,
            PwBadlistOpt::Upload { copt, .. } => copt.debug,
            PwBadlistOpt::Remove { copt, .. } => copt.debug,
        }
    }

    pub async fn exec(&self) {
        match self {
            PwBadlistOpt::Show(copt) => {
                let client = copt.to_client(OpType::Read).await;
                match client.system_password_badlist_get().await {
                    Ok(list) => {
                        for i in list {
                            println!("{i}");
                        }
                        eprintln!("--");
                        eprintln!("Success");
                    }
                    Err(e) => crate::handle_client_error(e, copt.output_mode),
                }
            }
            PwBadlistOpt::Upload {
                copt,
                paths,
                dryrun,
            } => {
                info!("pre-processing - this may take a while ...");

                let mut pwset: Vec<String> = Vec::new();

                for f in paths.iter() {
                    let mut file = match File::open(f) {
                        Ok(v) => v,
                        Err(e) => {
                            debug!(?e);
                            info!("Skipping file -> {:?}", f);
                            continue;
                        }
                    };
                    let mut contents = String::new();
                    if let Err(e) = file.read_to_string(&mut contents) {
                        error!("{:?} -> {:?}", f, e);
                        continue;
                    }
                    let mut inner_pw: Vec<_> =
                        contents.as_str().lines().map(str::to_string).collect();
                    pwset.append(&mut inner_pw);
                }

                debug!("Deduplicating pre-set ...");
                pwset.sort_unstable();
                pwset.dedup();

                info!("Have {} unique passwords to process", pwset.len());

                // Break the list into chunks per thread availability
                let task_handles: Vec<_> = pwset
                    .chunks(CHUNK_SIZE)
                    .map(|chunk| chunk.to_vec())
                    .map(|chunk| {
                        task::spawn_blocking(move || {
                            let x = chunk
                                .iter()
                                .filter(|v| {
                                    if v.len() < 10 {
                                        return false;
                                    }
                                    zxcvbn::zxcvbn(v.as_str(), &[]).score() >= Score::Four
                                })
                                .map(|s| s.to_string())
                                .collect::<Vec<_>>();
                            eprint!(".");
                            x
                        })
                    })
                    .collect();

                let mut filt_pwset = Vec::with_capacity(pwset.len());

                for task_handle in task_handles {
                    let Ok(mut results) = task_handle.await else {
                        error!("Failed to join a worker thread, unable to proceed");
                        return;
                    };
                    filt_pwset.append(&mut results);
                }

                filt_pwset.sort_unstable();

                info!(
                    "{} passwords passed zxcvbn, uploading ...",
                    filt_pwset.len()
                );

                if *dryrun {
                    for pw in filt_pwset {
                        println!("{pw}");
                    }
                } else {
                    let client = copt.to_client(OpType::Write).await;
                    match client.system_password_badlist_append(filt_pwset).await {
                        Ok(_) => println!("Success"),
                        Err(e) => handle_client_error(e, copt.output_mode),
                    }
                }
            } // End Upload
            PwBadlistOpt::Remove { copt, paths } => {
                let client = copt.to_client(OpType::Write).await;

                let mut pwset: Vec<String> = Vec::new();

                for f in paths.iter() {
                    let mut file = match File::open(f) {
                        Ok(v) => v,
                        Err(e) => {
                            debug!(?e);
                            info!("Skipping file -> {:?}", f);
                            continue;
                        }
                    };
                    let mut contents = String::new();
                    if let Err(e) = file.read_to_string(&mut contents) {
                        error!("{:?} -> {:?}", f, e);
                        continue;
                    }
                    let mut inner_pw: Vec<_> =
                        contents.as_str().lines().map(str::to_string).collect();
                    pwset.append(&mut inner_pw);
                }

                debug!("Deduplicating pre-set ...");
                pwset.sort_unstable();
                pwset.dedup();

                if pwset.is_empty() {
                    eprintln!("No entries to remove?");
                    return;
                }

                match client.system_password_badlist_remove(pwset).await {
                    Ok(_) => println!("Success"),
                    Err(e) => handle_client_error(e, copt.output_mode),
                }
            } // End Remove
        }
    }
}
