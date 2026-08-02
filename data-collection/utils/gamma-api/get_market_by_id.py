#!/usr/bin/env python3
"""
Function to fetch market data by ID from Gamma API /markets/{id} endpoint
"""

import requests
from typing import Dict, Any, List, Optional, Union


def get_market_by_id(
    market_id: Union[int, str],
    include_tag: Optional[bool] = None,
    fields: Optional[List[str]] = None
) -> Dict[str, Any]:
    """
    Fetch market data by ID from Gamma API /markets/{id} endpoint
    
    Args:
        market_id (int/str): The market ID to fetch (required)
        include_tag (bool, optional): Include tag information
        fields (List[str], optional): Specific fields to extract from response.
                                    If None, returns everything.
    
    Returns:
        Dict[str, Any]: Complete market data with all available fields
    
    Available Response Fields:
        
        Basic Info: id, question, conditionId, slug, description, outcomes
        Media: image, icon, twitterCardImage, imageOptimized, iconOptimized
        Classification: category, marketType, formatType, ammType
        Financial: liquidity, volume, fee, outcomePrices, volumeNum, liquidityNum
        Dates: startDate, endDate, createdAt, updatedAt, closedTime
        Status: active, closed, new, featured, archived, restricted
        Trading: marketMakerAddress, enableOrderBook, orderPriceMinTickSize, orderMinSize
        Volume Metrics: volume24hr, volume1wk, volume1mo, volume1yr
        AMM/CLOB Split: volumeAmm, volumeClob, liquidityAmm, liquidityClob
        AMM Volume: volume24hrAmm, volume1wkAmm, volume1moAmm, volume1yrAmm
        CLOB Volume: volume24hrClob, volume1wkClob, volume1moClob, volume1yrClob
        Bounds: lowerBound, upperBound, lowerBoundDate, upperBoundDate
        Axis: xAxisValue, yAxisValue, denominationToken
        Social: commentsEnabled, disqusThread, notificationsEnabled
        Sponsorship: sponsorName, sponsorImage
        Resolution: resolutionSource, resolvedBy, umaResolutionStatus
        UMA: umaEndDate, umaEndDateIso, umaBond, umaReward, umaResolutionStatuses
        Display: wideFormat, groupItemTitle, groupItemThreshold
        Gaming: gameStartTime, secondsDelay, teamAID, teamBID, gameId
        Trading Config: clobTokenIds, makerBaseFee, takerBaseFee, acceptingOrders
        Automation: readyForCron, hasReviewedDates, automaticallyResolved, automaticallyActive
        Market Group: marketGroup, curationOrder, groupItemRange
        Sports: sportsMarketType, line, score
        Metadata: createdBy, updatedBy, questionID, mailchimpTag
        Technical: fpmmLive, customLiveness, ready, funded, pastSlugs
        Pricing: lastTradePrice, bestBid, bestAsk, spread
        Price Changes: oneDayPriceChange, oneHourPriceChange, oneWeekPriceChange, oneMonthPriceChange, oneYearPriceChange
        Display Options: clearBookOnStart, chartColor, seriesColor, showGmpSeries, showGmpOutcome, manualActivation
        Risk: negRiskOther, rfqEnabled
        Rewards: rewardsMinSize, rewardsMaxSpread
        Timestamps: readyTimestamp, fundedTimestamp, acceptingOrdersTimestamp, eventStartTime
        Deployment: pendingDeployment, deploying, deployingTimestamp, scheduledDeploymentTimestamp
        
        Related Objects: events (array), categories (array), tags (array)
        
    Examples:
        # Get complete market data
        market = get_market_by_id(12345)
        
        # Get market with tag info
        market = get_market_by_id(12345, include_tag=True)
        
        # Get specific fields only
        market = get_market_by_id(12345, fields=["id", "question", "volume", "active"])
        
        # Get basic market info
        basic = get_market_by_id(
            12345, 
            fields=["id", "question", "description", "active", "closed", "volume"]
        )
        
        # Get financial data
        financial = get_market_by_id(
            12345,
            fields=["id", "question", "volume", "liquidity", "volume24hr", "outcomePrices"]
        )
        
        # Get trading data
        trading = get_market_by_id(
            12345,
            fields=["id", "question", "marketMakerAddress", "enableOrderBook", "orderMinSize"]
        )
    """
    # Base URL for Gamma API
    base_url = "https://gamma-api.polymarket.com"
    
    # Construct the endpoint URL with market ID
    url = f"{base_url}/markets/{market_id}"
    
    # Build query parameters
    params = {}
    
    if include_tag is not None:
        params['include_tag'] = 'true' if include_tag else 'false'
    
    # Set up headers
    headers = {
        'User-Agent': 'PolyTrading/1.0',
        'Accept': 'application/json'
    }
    
    try:
        # Make the request
        response = requests.get(url, headers=headers, params=params, timeout=15)
        response.raise_for_status()  # Raise an exception for bad status codes
        
        # Parse JSON response
        data = response.json()
        
        # If no specific fields requested, return everything
        if fields is None:
            return data
        
        # Extract only requested fields
        extracted_data = {}
        for field in fields:
            if field in data:
                extracted_data[field] = data[field]
            else:
                extracted_data[field] = None  # Field not found in response
        
        return extracted_data
        
    except requests.exceptions.HTTPError as e:
        if e.response.status_code == 404:
            raise ValueError(f"Market with ID '{market_id}' not found")
        else:
            raise Exception(f"HTTP error {e.response.status_code}: {e.response.text}")
    
    except requests.exceptions.RequestException as e:
        raise Exception(f"Network error: {str(e)}")
    
    except Exception as e:
        raise Exception(f"Unexpected error: {str(e)}")


# Convenience functions for common field groups
def get_market_basic_info(market_id: Union[int, str]) -> Dict[str, Any]:
    """Get basic market information (convenience function)"""
    basic_fields = [
        "id", "question", "description", "slug", "active", "closed", 
        "startDate", "endDate", "category", "marketType"
    ]
    return get_market_by_id(market_id, fields=basic_fields)


def get_market_financial_data(market_id: Union[int, str]) -> Dict[str, Any]:
    """Get market financial/trading data (convenience function)"""
    financial_fields = [
        "id", "question", "volume", "liquidity", "volumeNum", "liquidityNum",
        "volume24hr", "volume1wk", "volume1mo", "volume1yr", "outcomePrices",
        "fee", "lastTradePrice", "bestBid", "bestAsk", "spread"
    ]
    return get_market_by_id(market_id, fields=financial_fields)


def get_market_trading_config(market_id: Union[int, str]) -> Dict[str, Any]:
    """Get market trading configuration (convenience function)"""
    trading_fields = [
        "id", "question", "marketMakerAddress", "enableOrderBook", 
        "orderPriceMinTickSize", "orderMinSize", "clobTokenIds",
        "makerBaseFee", "takerBaseFee", "acceptingOrders", "ammType"
    ]
    return get_market_by_id(market_id, fields=trading_fields)


def get_market_volume_breakdown(market_id: Union[int, str]) -> Dict[str, Any]:
    """Get detailed volume breakdown (AMM vs CLOB) (convenience function)"""
    volume_fields = [
        "id", "question", "volume", "volumeAmm", "volumeClob",
        "volume24hr", "volume24hrAmm", "volume24hrClob",
        "volume1wk", "volume1wkAmm", "volume1wkClob",
        "volume1mo", "volume1moAmm", "volume1moClob",
        "volume1yr", "volume1yrAmm", "volume1yrClob"
    ]
    return get_market_by_id(market_id, fields=volume_fields)


def get_market_with_events(market_id: Union[int, str]) -> Dict[str, Any]:
    """Get market with related events data (convenience function)"""
    event_fields = [
        "id", "question", "description", "events", "active", "closed", 
        "volume", "liquidity", "category", "startDate", "endDate"
    ]
    return get_market_by_id(market_id, fields=event_fields)


def get_market_status(market_id: Union[int, str]) -> Dict[str, Any]:
    """Get market status and state information (convenience function)"""
    status_fields = [
        "id", "question", "active", "closed", "new", "featured", 
        "archived", "restricted", "ready", "funded", "fpmmLive",
        "acceptingOrders", "automaticallyActive", "automaticallyResolved"
    ]
    return get_market_by_id(market_id, fields=status_fields)


def get_market_price_data(market_id: Union[int, str]) -> Dict[str, Any]:
    """Get market pricing and price change data (convenience function)"""
    price_fields = [
        "id", "question", "outcomePrices", "lastTradePrice", "bestBid", "bestAsk",
        "oneDayPriceChange", "oneHourPriceChange", "oneWeekPriceChange", 
        "oneMonthPriceChange", "oneYearPriceChange", "spread"
    ]
    return get_market_by_id(market_id, fields=price_fields)


# Example usage and test function
if __name__ == "__main__":
    print("Testing Gamma API /markets/{id} endpoint")
    print("=" * 50)
    
    try:
        # You'll need to replace this with a real market ID
        test_market_id = 12345  # Replace with actual market ID
        
        print(f"Testing with market ID: {test_market_id}")
        print("-" * 30)
        
        # Test 1: Get basic market info
        print("1. Getting basic market info...")
        try:
            basic = get_market_basic_info(test_market_id)
            print(f"   Question: {basic.get('question', 'N/A')}")
            print(f"   Market ID: {basic.get('id', 'N/A')}")
            print(f"   Active: {basic.get('active', 'N/A')}")
            print(f"   Category: {basic.get('category', 'N/A')}")
        except ValueError as e:
            print(f"   Market not found: {e}")
            print("   Note: Update test_market_id with a real market ID")
            print("   Skipping remaining tests...")
            exit()
        
        # Test 2: Get financial data
        print("\n2. Getting financial data...")
        financial = get_market_financial_data(test_market_id)
        print(f"   Volume: {financial.get('volume', 'N/A')}")
        print(f"   Liquidity: {financial.get('liquidity', 'N/A')}")
        print(f"   24h Volume: {financial.get('volume24hr', 'N/A')}")
        print(f"   Last Trade Price: {financial.get('lastTradePrice', 'N/A')}")
        
        # Test 3: Get trading configuration
        print("\n3. Getting trading configuration...")
        trading = get_market_trading_config(test_market_id)
        print(f"   Market Maker: {trading.get('marketMakerAddress', 'N/A')}")
        print(f"   Order Book Enabled: {trading.get('enableOrderBook', 'N/A')}")
        print(f"   Min Order Size: {trading.get('orderMinSize', 'N/A')}")
        
        # Test 4: Get volume breakdown
        print("\n4. Getting volume breakdown...")
        volume = get_market_volume_breakdown(test_market_id)
        print(f"   Total Volume: {volume.get('volume', 'N/A')}")
        print(f"   AMM Volume: {volume.get('volumeAmm', 'N/A')}")
        print(f"   CLOB Volume: {volume.get('volumeClob', 'N/A')}")
        
        # Test 5: Get price data
        print("\n5. Getting price data...")
        prices = get_market_price_data(test_market_id)
        print(f"   Best Bid: {prices.get('bestBid', 'N/A')}")
        print(f"   Best Ask: {prices.get('bestAsk', 'N/A')}")
        print(f"   24h Price Change: {prices.get('oneDayPriceChange', 'N/A')}")
        
        # Test 6: Get specific fields
        print("\n6. Getting specific fields...")
        specific = get_market_by_id(
            test_market_id, 
            fields=["id", "question", "volume", "active", "outcomePrices"]
        )
        print(f"   Retrieved fields: {list(specific.keys())}")
        
        # Test 7: Get with tag information
        print("\n7. Getting with tag information...")
        with_tags = get_market_by_id(test_market_id, include_tag=True)
        if 'tags' in with_tags:
            tags = with_tags.get('tags', [])
            print(f"   Tags: {len(tags) if tags else 0}")
        
        print(f"   Total response fields: {len(with_tags.keys())}")
        
    except Exception as e:
        print(f"Error during test: {e}")
        print("\nNote: To test this function properly, you need to:")
        print("1. Find a real market ID from the API")
        print("2. Replace 'test_market_id = 12345' with the real ID")
        print("3. Run the test again")