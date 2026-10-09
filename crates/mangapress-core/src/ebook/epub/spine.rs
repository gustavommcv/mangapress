use crate::pipeline::spread::PageRole;

/// Which side of a two-page view each page belongs on — upstream's
/// `page-spread-*` assignment, for readers that show two pages at once.
///
/// Going forward, ordinary pages alternate, starting on the side reading
/// begins from (right for right-to-left). A split spread's halves take that
/// side and the opposite one, a rotated spread sits in the center, and after
/// either the alternation starts over. Then, going backward from the last
/// spread in the book, the ordinary pages *before* each spread are
/// re-assigned so that they alternate up to it and the spread's halves
/// always land on a fresh pair — without that second pass a spread preceded
/// by an odd number of pages would straddle two page-turns.
///
/// `start_on_second_side` flips where the very first page sits
/// (`--invertdirection`, `--spreadshift`, or both cancelling out);
/// `one_page_landscape` overrides everything with "center".
pub(super) fn page_spread_sides(
    roles: &[PageRole],
    right_to_left: bool,
    start_on_second_side: bool,
    one_page_landscape: bool,
) -> Vec<&'static str> {
    if one_page_landscape {
        return vec!["center"; roles.len()];
    }
    let (first_side, second_side) = if right_to_left {
        ("right", "left")
    } else {
        ("left", "right")
    };
    let flip = |side: &'static str| if side == "right" { "left" } else { "right" };

    let mut sides = Vec::with_capacity(roles.len());
    let mut side = if start_on_second_side {
        second_side
    } else {
        first_side
    };
    for role in roles {
        match role {
            PageRole::Normal => {
                sides.push(side);
                side = flip(side);
            }
            PageRole::SplitFirst => {
                sides.push(first_side);
                side = first_side;
            }
            PageRole::SplitSecond => {
                sides.push(second_side);
                side = first_side;
            }
            PageRole::Rotated => {
                sides.push("center");
                side = first_side;
            }
        }
    }

    let mut spread_seen = false;
    for (index, role) in roles.iter().enumerate().rev() {
        if *role != PageRole::Normal {
            spread_seen = true;
            side = second_side;
        } else if spread_seen {
            sides[index] = side;
            side = flip(side);
        }
    }
    sides
}
