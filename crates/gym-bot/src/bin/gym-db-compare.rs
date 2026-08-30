//! Lossless normalized gym database comparison CLI.

use std::path::PathBuf;

use gym_bot::parity::{DifferenceAllowList, compare_snapshots, normalize_database};

fn main() {
    let mut arguments = std::env::args_os().skip(1).map(PathBuf::from);
    let (Some(expected), Some(actual), None) =
        (arguments.next(), arguments.next(), arguments.next())
    else {
        eprintln!("usage: gym-db-compare <expected-gym.db> <actual-gym.db>");
        std::process::exit(2);
    };
    let result = normalize_database(&expected).and_then(|expected| {
        normalize_database(&actual)
            .and_then(|actual| compare_snapshots(&expected, &actual, &DifferenceAllowList::empty()))
    });
    match result {
        Ok(differences) if differences.is_empty() => println!("equal"),
        Ok(differences) => {
            println!("{differences:#?}");
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("gym database comparison failed: {error}");
            std::process::exit(2);
        }
    }
}
