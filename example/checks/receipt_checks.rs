//! Checks for the receipt renderer.
//!
//! Rust tests, in a directory with no special meaning, linked by annotation.

use crate::receipt::{render, CartLine};

// @tests REQ-RECEIPT.lines
#[test]
fn every_cart_line_is_rendered_with_its_own_price() {
    let lines = vec![
        CartLine { description: "Coffee beans, 1kg".into(), cents: 2_400 },
        CartLine { description: "Filter papers".into(), cents: 350 },
    ];
    let receipt = render(&lines, 2_750);
    assert_eq!(receipt.lines.len(), 2);
    assert!(receipt.lines[0].contains("24.00"));
    assert!(receipt.lines[1].contains("3.50"));
}

// There is no check for REQ-RECEIPT.audit. The clause is exempt — the reason is
// recorded at the exemption itself, in `service/receipt.rs` — so its absence
// here is accounted for rather than merely unnoticed.
