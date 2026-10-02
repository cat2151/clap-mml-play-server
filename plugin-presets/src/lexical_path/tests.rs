use super::*;

#[test]
fn relative_path_becomes_absolute_without_touching_the_file_system() {
    let path = lexical_absolute(Path::new("missing_dir_for_lexical_path/Missing.sfz")).unwrap();

    assert!(path.is_absolute());
    assert!(path.ends_with("missing_dir_for_lexical_path/Missing.sfz"));
}

#[cfg(windows)]
#[test]
fn verbatim_prefix_is_removed_so_it_matches_a_plain_root() {
    let path = lexical_absolute(Path::new(r"\\?\D:\Samples\sfz\Bank\Programs\Full.sfz")).unwrap();

    assert_eq!(path, Path::new(r"D:\Samples\sfz\Bank\Programs\Full.sfz"));
}

#[cfg(windows)]
#[test]
fn parent_dir_is_folded_on_windows() {
    let path = lexical_absolute(Path::new(r"D:\Samples\sfz\Bank\..\Other\Full.sfz")).unwrap();

    assert_eq!(path, Path::new(r"D:\Samples\sfz\Other\Full.sfz"));
}
