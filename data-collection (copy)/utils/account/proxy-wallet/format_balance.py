"""
Format balance from cents to human-readable string
"""
def format_balance(balance_cents: int) -> str:
    """
    Format balance from cents to human-readable string
    Args:
        balance_cents: Balance in cents
    Returns:
        str: Formatted balance (e.g., "1.50 USDC")
    """
    usdc_amount = balance_cents / 100
    return f"{usdc_amount:.2f} USDC"
