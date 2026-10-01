use std::{ffi::OsString, path::PathBuf};

use rstest::rstest;

use super::{Config, InputConfig};

#[rstest]
#[case(&[], 0)]
#[case(&["one.csv"], 1)]
#[case(&["one.csv", "two.csv"], 2)]
fn selects_streaming_or_batch_execution(#[case] paths: &[&str], #[case] count: usize) {
    match Config::from_args(paths.iter().map(OsString::from)) {
        Err(_) => assert_eq!(count, 0),
        Ok(Config::Single(config)) => {
            assert_eq!(count, 1);
            assert_eq!(config.input_path, PathBuf::from(paths[0]));
        }
        Ok(Config::Batch(config)) => {
            assert_eq!(config.input_paths.len(), count);
            assert!(config.workers.get() >= 1);
            assert_eq!(
                config.input_paths,
                paths.iter().map(PathBuf::from).collect::<Vec<_>>()
            );
        }
    }
}

#[rstest]
#[case(&[], None)]
#[case(&["one.csv", "two.csv"], None)]
#[case(&["one.csv"], Some("one.csv"))]
#[case(&["path with spaces.csv"], Some("path with spaces.csv"))]
fn accepts_exactly_one_path(#[case] args: &[&str], #[case] expected: Option<&str>) {
    let result = InputConfig::from_args(args.iter().map(OsString::from));
    match expected {
        Some(path) => assert_eq!(result.unwrap().input_path, PathBuf::from(path)),
        None => assert!(result.unwrap_err().to_string().contains("Usage:")),
    }
}

#[cfg(windows)]
#[test]
fn preserves_non_unicode_path() {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    let units = [0xD800, 0x002E, 0x0063, 0x0073, 0x0076];
    let config = InputConfig::from_args([OsString::from_wide(&units)]).unwrap();
    assert_eq!(
        config
            .input_path
            .as_os_str()
            .encode_wide()
            .collect::<Vec<_>>(),
        units
    );
}
