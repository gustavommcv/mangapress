use super::*;
use std::path::PathBuf;

#[test]
fn numeric_runs_compare_numerically_not_lexicographically() {
    assert_eq!(compare("page2.jpg", "page10.jpg"), Ordering::Less);
}

#[test]
fn equal_strings_compare_equal() {
    assert_eq!(compare("page002.jpg", "page002.jpg"), Ordering::Equal);
}

#[test]
fn case_does_not_affect_the_order() {
    // Upstream's `walkSort()` on these names gives exactly this order.
    let mut names = vec!["Chapter 10", "chapter 2", "Chapter 1", "b.jpg", "A.jpg"];
    names.sort_by(|a, b| compare(a, b));
    assert_eq!(
        names,
        ["A.jpg", "b.jpg", "Chapter 1", "chapter 2", "Chapter 10"]
    );
}

#[test]
fn purely_textual_strings_fall_back_to_lexicographic() {
    assert_eq!(compare("apple.jpg", "banana.jpg"), Ordering::Less);
}

#[test]
fn chapter_paths_sort_by_directory_first_then_by_file() {
    let a = PathBuf::from("c001 - Title One/page002.jpg");
    let b = PathBuf::from("c002 - Title Two/page001.jpg");
    assert_eq!(compare_paths(&a, &b), Ordering::Less);
}

#[test]
fn double_digit_chapter_sorts_after_single_digit() {
    let a = PathBuf::from("c2 - Two/page001.jpg");
    let b = PathBuf::from("c10 - Ten/page001.jpg");
    assert_eq!(compare_paths(&a, &b), Ordering::Less);
}

#[test]
fn a_folders_own_pages_come_before_the_folders_inside_it() {
    let mut paths = vec![
        "Chapter 2/001.png",
        "zz-credits.png",
        "Chapter 1/002.png",
        "cover.png",
        "Chapter 1/001.png",
    ];
    paths.sort_by(|a, b| compare_paths(Path::new(a), Path::new(b)));
    assert_eq!(
        paths,
        [
            "cover.png",
            "zz-credits.png",
            "Chapter 1/001.png",
            "Chapter 1/002.png",
            "Chapter 2/001.png"
        ]
    );
}

#[test]
fn the_same_holds_one_level_down() {
    let mut paths = vec![
        "Vol 1/Ch 2/1.png",
        "Vol 1/zz.png",
        "Vol 1/Ch 1/1.png",
        "Vol 1/intro.png",
    ];
    paths.sort_by(|a, b| compare_paths(Path::new(a), Path::new(b)));
    assert_eq!(
        paths,
        [
            "Vol 1/intro.png",
            "Vol 1/zz.png",
            "Vol 1/Ch 1/1.png",
            "Vol 1/Ch 2/1.png"
        ]
    );
}

#[test]
fn a_file_comes_before_a_folder_even_when_it_sorts_after_it_by_name() {
    assert_eq!(
        compare_paths(Path::new("z.png"), Path::new("a/1.png")),
        Ordering::Less
    );
    assert_eq!(
        compare_paths(Path::new("a/1.png"), Path::new("z.png")),
        Ordering::Greater
    );
}

#[test]
fn files_among_themselves_and_folders_among_themselves_keep_the_natural_order() {
    assert_eq!(
        compare_paths(Path::new("page2.png"), Path::new("page10.png")),
        Ordering::Less
    );
    assert_eq!(
        compare_paths(Path::new("Ch 2/1.png"), Path::new("Ch 10/1.png")),
        Ordering::Less
    );
}

fn sorted<'a>(names: &[&'a str]) -> Vec<&'a str> {
    let mut names = names.to_vec();
    names.sort_by(|a, b| compare(a, b));
    names
}

#[test]
fn a_name_comes_before_the_names_that_continue_it() {
    let names = ["p01-2.png", "p01 (2).png", "p01.png", "p01_b.png"];
    assert_eq!(
        sorted(&names),
        ["p01.png", "p01 (2).png", "p01-2.png", "p01_b.png"]
    );
    assert_eq!(
        sorted(&["cover2.png", "cover.png"]),
        ["cover.png", "cover2.png"]
    );
}

#[test]
fn a_number_inside_a_name_is_not_an_extension() {
    assert_eq!(
        sorted(&["2.png", "1.10.png", "1.5.png", "1.png"]),
        ["1.png", "1.5.png", "1.10.png", "2.png"]
    );
}

#[test]
fn digits_of_every_script_are_numbers() {
    assert_eq!(
        sorted(&["１０.png", "3.png", "２.png", "1.png"]),
        ["1.png", "２.png", "3.png", "１０.png"]
    );
    assert_eq!(
        sorted(&["第１０話", "第２話", "第１話"]),
        ["第１話", "第２話", "第１０話"]
    );
    // Digits of two scripts next to each other are one number.
    assert_eq!(compare("1２.png", "12.png"), Ordering::Equal);
}

#[test]
fn a_character_that_stands_for_a_digit_is_a_number_by_itself() {
    assert_eq!(
        sorted(&["x3.png", "x².png", "x1.png"]),
        ["x1.png", "x².png", "x3.png"]
    );
    // A circled ten is not a digit.
    assert_eq!(compare("x⑩.png", "x9.png"), Ordering::Greater);
}

#[test]
fn only_short_extensions_are_split_off_and_at_most_two() {
    assert_eq!(extensions("a.tar.gz"), [".tar", ".gz"]);
    assert_eq!(extensions("a.b.c.d.png"), [".d", ".png"]);
    assert_eq!(extensions("a.5.png"), [".png"]);
    assert_eq!(extensions("a.longer"), Vec::<String>::new());
    assert_eq!(extensions(".png"), Vec::<String>::new());
    assert_eq!(extensions("Vol. 1"), [". 1"]);
}

#[test]
fn numbers_of_any_size_compare_by_value() {
    let big = "9".repeat(40);
    let bigger = format!("1{}", "0".repeat(40));
    assert_eq!(
        compare(&format!("p{big}.png"), &format!("p{bigger}.png")),
        Ordering::Less
    );
    assert_eq!(compare("p007.png", "p7.png"), Ordering::Equal);
}

#[test]
fn equal_paths_compare_equal_and_a_prefix_comes_first() {
    assert_eq!(
        compare_paths(Path::new("a/1.png"), Path::new("a/1.png")),
        Ordering::Equal
    );
    assert_eq!(
        compare_paths(Path::new("a"), Path::new("a/1.png")),
        Ordering::Less
    );
}
