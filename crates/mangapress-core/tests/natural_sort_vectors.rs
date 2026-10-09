//! The order of file names that `natsort` gives (as KCC 12.0.0 uses it), kept in
//! `natural_sort_vectors.txt` by `tools/parity/natsort_vectors.py`.

use mangapress_core::natural_sort::compare;
use std::cmp::Ordering;

const VECTORS: &str = include_str!("natural_sort_vectors.txt");

fn groups() -> Vec<Vec<&'static str>> {
    VECTORS
        .split("---\n")
        .skip(1)
        .map(|group| group.lines().filter(|line| !line.is_empty()).collect())
        .collect()
}

#[test]
fn the_file_holds_the_groups_it_says() {
    assert!(groups().len() > 50);
    assert!(groups().iter().all(|group| group.len() > 1));
}

#[test]
fn each_name_comes_before_the_next_in_its_group() {
    for group in groups() {
        for pair in group.windows(2) {
            assert_eq!(
                compare(pair[0], pair[1]),
                Ordering::Less,
                "{:?} should come before {:?} (group of {:?})",
                pair[0],
                pair[1],
                group
            );
        }
    }
}

#[test]
fn sorting_a_group_back_to_front_gives_the_order_natsort_gives() {
    for group in groups() {
        let mut names: Vec<&str> = group.iter().rev().copied().collect();
        names.sort_by(|a, b| compare(a, b));
        assert_eq!(names, group);
    }
}
