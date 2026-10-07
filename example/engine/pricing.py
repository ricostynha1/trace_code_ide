"""Checkout pricing.

The implementation is Python and the reference model is Lean. Nothing in
TraceLean needs to relate the two languages: they are bound behaviourally, by
running both over the same random inputs and comparing the answers, which is why
the implementation can be in whatever language the job actually calls for.

Note where the annotations live — on the functions, in ordinary comments. There
is no `src/` rule, no filename convention, and no registry file listing what
implements what. Move this file anywhere and the links move with it.
"""

FREE_SHIPPING_THRESHOLD_CENTS = 10_000
FLAT_SHIPPING_CENTS = 599
REMOTE_SURCHARGE_CENTS = 400

# Stand-in for the carrier's remote-area table. Being a hard-coded list is
# exactly why the link below carries a partial qualifier.
REMOTE_POSTCODES = {"4750-000", "9900-100", "9950-200"}


# @implements REQ-DISCOUNT.tiers
# @implements REQ-DISCOUNT.rounding
def discount_cents(subtotal_cents: int) -> int:
    """Tiered discount, rounded down to the cent.

    Integer floor division does the rounding, matching the model. Using
    `round()` or float multiplication here would agree with the model on most
    inputs and disagree on the ones where a cent is at stake — findable by
    differential testing, invisible to a handful of examples.
    """
    if subtotal_cents >= 20_000:
        return subtotal_cents * 15 // 100
    if subtotal_cents >= 5_000:
        return subtotal_cents * 10 // 100
    return 0


# @implements REQ-SHIPPING.free
# @implements REQ-SHIPPING.flat
# @implements REQ-SHIPPING.remote
# @partial reason="remote areas come from a hard-coded postcode list, not the carrier table"
def shipping_cents(after_discount_cents: int, remote: bool) -> int:
    """Shipping, decided on the discounted subtotal.

    The argument name is load-bearing: passing the *pre*-discount subtotal here
    is the bug this requirement exists to prevent, and it only shows up near the
    free-shipping boundary.
    """
    base = 0 if after_discount_cents >= FREE_SHIPPING_THRESHOLD_CENTS else FLAT_SHIPPING_CENTS
    return base + (REMOTE_SURCHARGE_CENTS if remote else 0)


DISCOUNT_CAP_CENTS = 5_000


# @implements REQ-DISCOUNT.cap
def capped_discount_cents(subtotal_cents: int) -> int:
    """The tier discount, capped.

    This is the planted defect of the example, and it is planted the way real
    ones arrive: the constant is defined, the function is named for what it is
    meant to do, and the line that would use the cap is simply not here. Reading
    this file alone gives no reason for suspicion, every test in `checks/`
    passes, and the differential run reports a divergence on its first hundred
    cases.
    """
    return discount_cents(subtotal_cents)


def is_remote(postcode: str) -> bool:
    return postcode in REMOTE_POSTCODES


# @implements REQ-CHECKOUT.total
def price(subtotal_cents: int, remote: bool) -> dict:
    """The three decisions, returned separately.

    Returning the parts rather than one number is what lets a differential-test
    divergence say *which* decision was wrong instead of only that the totals
    disagree.
    """
    discount = discount_cents(subtotal_cents)
    after_discount = subtotal_cents - discount
    shipping = shipping_cents(after_discount, remote)
    return {
        "discountCents": discount,
        "shippingCents": shipping,
        "totalCents": after_discount + shipping,
    }


# REQ-DISCOUNT.coupon has no implementation on purpose. There is deliberately no
# annotation here claiming otherwise: an annotation is a claim, and writing one
# for code that does not exist is the single thing this system must never make
# easy. The panel reports the clause as unbacked, which is the truth.
