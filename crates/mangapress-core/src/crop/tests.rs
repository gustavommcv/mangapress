use super::*;

#[test]
fn group_close_values_merges_within_tolerance_and_splits_beyond_it() {
    assert_eq!(
        group_close_values(&[1, 2, 3, 100, 101], 5.0),
        vec![(1, 3), (101, 101)]
    );
}

#[test]
fn group_close_values_of_empty_input_is_empty() {
    assert_eq!(group_close_values(&[], 5.0), vec![]);
}

#[test]
fn default_power_matches_kcc_default() {
    // KCC's --croppingpower default is 1.0 -> threshold 176.
    assert_eq!(threshold_from_power(1.0), 176.0);
}

#[test]
fn get_bbox_of_blank_image_is_none() {
    let img = GrayImage::from_pixel(20, 20, Luma([0]));
    assert_eq!(get_bbox(&img), None);
}

#[test]
fn get_bbox_finds_tight_rectangle() {
    let mut img = GrayImage::from_pixel(20, 20, Luma([0]));
    for y in 5..10 {
        for x in 3..8 {
            img.put_pixel(x, y, Luma([255]));
        }
    }
    assert_eq!(
        get_bbox(&img),
        Some(Bbox {
            left: 3,
            top: 5,
            right: 8,
            bottom: 10
        })
    );
}

// 1000x1000 so 2%/2.5% of each dimension round to different pixel
// counts (20 vs 25) — on a too-small canvas upstream's own degenerate
// guard skips the whole function, which would make these tests
// vacuously pass no matter what they assert.
const EDGE_TEST_DIM: u32 = 1000;

#[test]
fn ignore_pixels_near_edge_clears_sparse_border_noise() {
    let mut img = GrayImage::from_pixel(EDGE_TEST_DIM, EDGE_TEST_DIM, Luma([0]));
    // A single stray foreground pixel right at the raw top edge, with
    // nothing in the inner 2%-2.5% band just past it (density 0 there).
    // Upstream reads the empty inner band as "content doesn't reach
    // this far in", then wipes the whole outer edge strip because it
    // has *any* foreground pixel at all.
    img.put_pixel(500, 0, Luma([255]));
    ignore_pixels_near_edge(&mut img);
    assert_eq!(img.get_pixel(500, 0)[0], 0);
}

#[test]
fn ignore_pixels_near_edge_keeps_dense_border_content() {
    let mut img = GrayImage::from_pixel(EDGE_TEST_DIM, EDGE_TEST_DIM, Luma([0]));
    // Fill the top edge strip *and* the inner 2%-2.5% band just past it
    // — real content reaching that far in, not scan noise, so upstream
    // must leave both untouched.
    for y in 0..30 {
        for x in 0..EDGE_TEST_DIM {
            img.put_pixel(x, y, Luma([255]));
        }
    }
    ignore_pixels_near_edge(&mut img);
    assert_eq!(img.get_pixel(500, 0)[0], 255);
}

#[test]
fn large_margin_is_capped_to_ten_percent() {
    let bbox = Bbox {
        left: 60,
        top: 90,
        right: 140,
        bottom: 210,
    };
    let crop =
        apply_policy(bbox, (200, 300), &CropPolicy::default()).expect("a crop should be produced");
    assert_eq!(
        crop,
        CropBox {
            left: 20,
            top: 30,
            right: 180,
            bottom: 270
        }
    );
}

#[test]
fn minimum_area_ratio_suppresses_crop_when_too_aggressive() {
    let bbox = Bbox {
        left: 60,
        top: 90,
        right: 140,
        bottom: 210,
    };
    // The capped crop keeps (180-20)*(270-30)=38400 of 60000 px = 64%.
    let crop = apply_policy(
        bbox,
        (200, 300),
        &CropPolicy {
            minimum_area_ratio: 0.9,
            ..Default::default()
        },
    );
    assert_eq!(crop, None);
}

#[test]
fn preserve_margin_backs_off_the_crop() {
    let full = FractionalBox([20.0, 30.0, 180.0, 270.0]);
    let backed_off = apply_preserve_margin(full, (200, 300), 50.0);
    assert_eq!(
        backed_off.rounded(),
        CropBox {
            left: 10,
            top: 15,
            right: 190,
            bottom: 285
        }
    );
}

#[test]
fn preserve_margin_zero_is_identity() {
    let full = FractionalBox([20.0, 30.0, 180.0, 270.0]);
    assert_eq!(apply_preserve_margin(full, (200, 300), 0.0), full);
}

#[test]
fn a_capped_edge_on_a_half_pixel_rounds_to_the_even_neighbour() {
    // 10% of a 905px-wide page is 90.5: Pillow's crop rounds that to 90
    // (even), and the matching right edge, 814.5, to 814. The detected
    // content box is narrower than the cap on every side, so the cap is
    // what decides all four edges.
    let bbox = Bbox {
        left: 300,
        top: 300,
        right: 600,
        bottom: 600,
    };
    let crop = apply_policy(bbox, (905, 1005), &CropPolicy::default()).unwrap();
    assert_eq!(
        crop,
        CropBox {
            left: 90,
            top: 100,
            right: 814,
            bottom: 904
        }
    );
}

#[test]
fn apply_crop_produces_expected_dimensions() {
    let img = GrayImage::from_pixel(200, 300, Luma([255]));
    let cropped = apply_crop(
        &img,
        CropBox {
            left: 20,
            top: 30,
            right: 180,
            bottom: 270,
        },
    );
    assert_eq!(cropped.dimensions(), (160, 240));
}
