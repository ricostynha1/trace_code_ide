"""Example-based checks for pricing.

These sit in `checks/`, not in `tests/unit/`, and nothing breaks. TraceLean used
to find tests by path pattern; it now finds them because each one says which
clause it exercises. That is the difference between a tool that works on your
project and a tool your project has to be rearranged for.

Run: python3 -m unittest discover -s checks -p '*_checks.py'
"""
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "engine"))

import pricing  # noqa: E402


class DiscountChecks(unittest.TestCase):
    # @tests REQ-DISCOUNT.tiers
    def test_bands_are_half_open_and_ascending(self):
        self.assertEqual(pricing.discount_cents(4_999), 0)
        self.assertEqual(pricing.discount_cents(5_000), 500)
        self.assertEqual(pricing.discount_cents(19_999), 1_999)
        self.assertEqual(pricing.discount_cents(20_000), 3_000)

    # @tests REQ-DISCOUNT.rounding
    def test_a_fractional_cent_rounds_toward_the_customer(self):
        # 10% of 5,009 is 500.9 cents; the customer is charged the extra 0.9.
        self.assertEqual(pricing.discount_cents(5_009), 500)


class ShippingChecks(unittest.TestCase):
    # @tests REQ-SHIPPING.free
    def test_free_above_the_threshold(self):
        self.assertEqual(pricing.shipping_cents(10_000, remote=False), 0)

    # @tests REQ-SHIPPING.flat
    def test_flat_below_the_threshold(self):
        self.assertEqual(pricing.shipping_cents(9_999, remote=False), 599)

    # @tests REQ-SHIPPING.remote
    def test_remote_surcharge_applies_even_when_shipping_is_free(self):
        self.assertEqual(pricing.shipping_cents(50_000, remote=True), 400)


class CapChecks(unittest.TestCase):
    # @tests REQ-DISCOUNT.cap
    def test_a_small_order_is_under_the_cap(self):
        """Passes, and proves nothing about the clause it is annotated against.

        The cap is 5,000 cents and this order discounts by 1,000, so the capped
        and uncapped answers are identical here. `engine/pricing.py` never
        applies the cap at all, and this test is green anyway.

        That is the argument for differential testing in one method: an
        example-based test can only fail where somebody thought to look, and the
        person who writes the test is the same person who missed the case in the
        implementation. The differential run finds it above 33,334 cents without
        anyone having thought of that number.
        """
        self.assertEqual(pricing.capped_discount_cents(10_000), 1_000)


class TotalChecks(unittest.TestCase):
    # @tests REQ-CHECKOUT.total
    def test_the_discount_can_drop_an_order_out_of_free_shipping(self):
        # 10,500 discounts by 1,050 to 9,450 — below the free-shipping line.
        # An implementation that decided shipping *before* the discount would
        # return 0 here, and would pass every other check in this file.
        result = pricing.price(10_500, remote=False)
        self.assertEqual(result["discountCents"], 1_050)
        self.assertEqual(result["shippingCents"], 599)
        self.assertEqual(result["totalCents"], 10_049)


if __name__ == "__main__":
    unittest.main()
