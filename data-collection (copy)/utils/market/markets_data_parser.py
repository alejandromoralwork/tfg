"""
Markets Data Parser - Single Markets
Fetches and parses single/standalone market data from Polymarket Gamma API
"""

import logging
import json
import requests
import re
from typing import List, Dict, Any, Optional

# Set up logging
logger = logging.getLogger(__name__)


def _normalize_clob_token_ids(raw) -> List[str]:
    """Normalize various clobTokenIds formats into a list of string ids.

    The API sometimes returns this field as a list, or as a JSON-encoded string.
    This helper will attempt to return a clean list of string ids in all cases.
    """
    if not raw:
        return []

    # If it's already a list, convert items to str
    if isinstance(raw, list):
        return [str(x) for x in raw]

    # If it's a string that looks like a JSON list, try to parse it
    if isinstance(raw, str):
        raw_str = raw.strip()
        try:
            if raw_str.startswith('[') and raw_str.endswith(']'):
                parsed = json.loads(raw_str)
                if isinstance(parsed, list):
                    return [str(x) for x in parsed]
        except Exception:
            # Fall through and try to split by commas as a last resort
            pass

        # Last-resort: split on commas and strip
        parts = [p.strip().strip('"').strip('\'') for p in raw_str.split(',') if p.strip()]
        return [p for p in parts if p]

    # Unknown type - convert to string
    return [str(raw)]


class MarketsDataParser:
    """
    Parser for single market data from Polymarket Gamma API
    """
    
    def __init__(self, api_url: str = "https://gamma-api.polymarket.com/markets"):
        """
        Initialize the markets data parser
        
        Args:
            api_url: Polymarket Gamma API URL for markets
        """
        self.api_url = api_url
        self.querystrings = {
            "active": "true",
            "closed": "false",
            "archived": "false"  # Also exclude archived markets
        }
    
    def get_markets(self, additional_params: Optional[Dict[str, str]] = None) -> List[Dict[str, Any]]:
        """
        Fetch and parse active markets from Polymarket with pagination support
        
        Args:
            additional_params: Additional query parameters
            
        Returns:
            List of market dictionaries with id, outcomePrices, and slug
        """
        try:
            # Merge additional parameters if provided
            params = self.querystrings.copy()
            if additional_params:
                params.update(additional_params)
            
            all_markets = []
            limit = 100
            offset = 0
            
            # Add pagination parameters if not already specified
            if 'limit' not in params:
                params['limit'] = str(limit)
            if 'offset' not in params:
                params['offset'] = str(offset)
            
            while True:
                # Update offset for this iteration
                params['offset'] = str(offset)
                
                logger.info(f"Fetching markets from {self.api_url} (offset: {offset})")
                response = requests.get(self.api_url, params=params, timeout=30)
                response.raise_for_status()
                
                markets_data = response.json()
                logger.info(f"Retrieved {len(markets_data)} markets from API")
                
                if not markets_data:
                    # No more markets, break the loop
                    break
                
                # Parse this batch
                parsed_batch = self._parse_markets(markets_data)
                all_markets.extend(parsed_batch)
                
                # If we got fewer markets than requested, we're on the last page
                if len(markets_data) < int(params['limit']):
                    break
                
                # Move to next page
                offset += len(markets_data)
                
                # Safety check to avoid infinite loops
                if offset > 10000:
                    logger.warning("Reached safety limit of 10000 markets")
                    break
            
            logger.info(f"Successfully fetched {len(all_markets)} total markets")
            return all_markets
            
        except requests.RequestException as e:
            logger.error(f"Failed to fetch markets: {e}")
            raise
        except json.JSONDecodeError as e:
            logger.error(f"Failed to parse JSON response: {e}")
            raise
    
    def _parse_markets(self, markets_data: List[Dict]) -> List[Dict[str, Any]]:
        """
        Parse raw market data and extract relevant information
        
        Args:
            markets_data: Raw market data from API
            
        Returns:
            Parsed market data
        """
        decoded_markets = []
        
        for market in markets_data:
            # Check if market is active before parsing
            market_active = market.get("active", True)
            market_closed = market.get("closed", False)
            
            # Skip inactive or closed markets
            if not market_active or market_closed:
                logger.debug(f"Skipping inactive/closed market: {market.get('id')}")
                continue
            
            parsed_market = self._parse_single_market(market)
            if parsed_market:
                decoded_markets.append(parsed_market)
        
        logger.info(f"Successfully parsed {len(decoded_markets)} markets")
        return decoded_markets
    
    def _parse_single_market(self, market: Dict) -> Optional[Dict[str, Any]]:
        """
        Parse a single market and extract outcome prices
        
        Args:
            market: Single market data
            
        Returns:
            Parsed market dict or None if parsing fails
        """
        try:
            outcome_prices = market.get("outcomePrices")
            if not outcome_prices:
                logger.debug(f"Market {market.get('id')} has no outcome prices")
                return None
            
            # Parse outcome prices - expecting format like ["0.52", "0.48"]
            outcome_prices_str = str(outcome_prices)
            match = re.search(r'\[\"([0-9]+\.[0-9]+)\", \"([0-9]+\.[0-9]+)\"\]', outcome_prices_str.strip())
            
            if match:
                parsed_prices = [float(match.group(1)), float(match.group(2))]
                
                # Safely parse volume and liquidity
                try:
                    volume = market.get("volume", 0)
                    volume = float(volume) if volume is not None else 0
                except (ValueError, TypeError):
                    volume = 0
                
                try:
                    liquidity = market.get("liquidity", 0)
                    liquidity = float(liquidity) if liquidity is not None else 0
                except (ValueError, TypeError):
                    liquidity = 0
                
                return {
                    "id": market.get("id"),
                    "slug": market.get("slug"),
                    "outcomePrices": parsed_prices,
                    "question": market.get("question", ""),
                    "description": market.get("description", ""),
                    "endDate": market.get("endDate"),
                    "volume": volume,
                    "liquidity": liquidity,
                    "clobTokenIds": _normalize_clob_token_ids(market.get("clobTokenIds", []))
                }
            else:
                logger.debug(f"Could not parse outcome prices for market {market.get('id')}")
                return None
                
        except Exception as e:
            logger.warning(f"Error parsing market {market.get('id', 'unknown')}: {e}")
            return None
    
    def get_market_by_id(self, market_id: str) -> Optional[Dict[str, Any]]:
        """
        Get a specific market by its ID
        
        Args:
            market_id: Market ID to fetch
            
        Returns:
            Market data or None if not found
        """
        markets = self.get_markets()
        for market in markets:
            if market["id"] == market_id:
                return market
        return None
    
    def get_markets_by_volume(self, min_volume: float = 0) -> List[Dict[str, Any]]:
        """
        Get markets filtered by minimum volume
        
        Args:
            min_volume: Minimum volume threshold
            
        Returns:
            List of markets with volume >= min_volume
        """
        markets = self.get_markets()
        filtered_markets = []
        
        for market in markets:
            try:
                volume = market.get("volume", 0)
                # Convert to float if it's a string
                if isinstance(volume, str):
                    volume = float(volume)
                elif volume is None:
                    volume = 0
                
                if volume >= min_volume:
                    filtered_markets.append(market)
            except (ValueError, TypeError):
                # Skip markets with invalid volume data
                continue
        
        return filtered_markets


# Example usage
if __name__ == "__main__":
    import csv
    import os
    import requests
    import json
    import re
    from datetime import datetime
    
    # Set up logging
    logging.basicConfig(level=logging.INFO)
    
    def get_all_active_markets():
        """
        Get ALL active markets using the recommended API approach:
        1. Use /events endpoint as primary source (more efficient)
        2. Work backwards from events to get their markets
        3. Supplement with standalone markets from /markets endpoint
        
        Following official API recommendations with proper parameters
        """
        all_markets = []
        
        try:
            # 1. PRIMARY: Get ALL markets from events (recommended approach)
            print("🔍 Fetching ALL markets from events (primary source)...")
            
            events_api_url = "https://gamma-api.polymarket.com/events"
            all_events = []
            limit = 100
            offset = 0
            
            # Get all events with proper parameters (following API docs)
            while True:
                try:
                    params = {
                        "order": "id",          # Order by event ID
                        "ascending": "false",   # Get newest events first
                        "closed": "false",      # Only active markets
                        "limit": str(limit),    # Control response size
                        "offset": str(offset)   # For pagination
                    }
                    
                    print(f"   Fetching events: offset {offset}, limit {limit} (order=id, ascending=false)")
                    response = requests.get(events_api_url, params=params, timeout=30)
                    response.raise_for_status()
                    
                    batch_events = response.json()
                    print(f"   Retrieved {len(batch_events)} events in this batch")
                    
                    if not batch_events:
                        break
                    
                    all_events.extend(batch_events)
                    
                    # If we got fewer than limit, we're on the last page
                    if len(batch_events) < limit:
                        break
                    
                    offset += len(batch_events)
                    
                    # Safety check
                    if offset > 10000:
                        print("   Reached safety limit for events")
                        break
                        
                except Exception as e:
                    print(f"   Error in events batch at offset {offset}: {e}")
                    break
            
            print(f"   Found {len(all_events)} total active events")
            
            # Extract all markets from all events
            event_markets_count = 0
            
            for event in all_events:
                # Double-check event is active (API should filter but be safe)
                event_active = event.get("active", True)
                event_closed = event.get("closed", False)
                
                if not event_active or event_closed:
                    continue
                
                # Capture event information for grouping
                event_context = {
                    'parent_event_id': event.get('id'),
                    'parent_event_slug': event.get('slug'),
                    'parent_event_title': event.get('title', ''),
                    'parent_event_description': event.get('description', ''),
                    'event_tags': event.get('tags', [])
                }
                
                markets = event.get("markets", [])
                
                # Process ALL markets in event
                for market in markets:
                    # Check if individual market is active
                    market_active = market.get("active", True)
                    market_closed = market.get("closed", False)
                    
                    if not market_active or market_closed:
                        continue
                    
                    # Parse market outcome prices
                    outcome_prices = market.get("outcomePrices")
                    if not outcome_prices:
                        continue
                    
                    outcome_prices_str = str(outcome_prices)
                    match = re.search(r'\[\"([0-9]+\.[0-9]+)\", \"([0-9]+\.[0-9]+)\"\]', outcome_prices_str)
                    
                    if match:
                        parsed_prices = [float(match.group(1)), float(match.group(2))]
                        
                        # Safely parse volume and liquidity
                        try:
                            volume = market.get("volume", 0)
                            volume = float(volume) if volume is not None else 0
                        except (ValueError, TypeError):
                            volume = 0
                        
                        try:
                            liquidity = market.get("liquidity", 0)
                            liquidity = float(liquidity) if liquidity is not None else 0
                        except (ValueError, TypeError):
                            liquidity = 0
                        
                        parsed_market = {
                            "id": market.get("id"),
                            "slug": market.get("slug"),
                            "outcomePrices": parsed_prices,
                            "question": market.get("question", ""),
                            "description": market.get("description", ""),
                            "endDate": market.get("endDate"),
                            "volume": volume,
                            "liquidity": liquidity,
                            "source": "event",
                            "questionID": market.get("questionID"),
                            "conditionId": market.get("conditionId"),
                            "clobTokenIds": _normalize_clob_token_ids(market.get("clobTokenIds", [])),
                            "parent_event_id": event_context['parent_event_id'],
                            "parent_event_slug": event_context['parent_event_slug'],
                            "parent_event_title": event_context['parent_event_title'],
                            "parent_event_description": event_context['parent_event_description'],
                            "event_tags": event_context['event_tags']
                        }
                        
                        all_markets.append(parsed_market)
                        event_markets_count += 1
            
            print(f"   Found {len(event_markets_count)} markets from all events (primary source)")
            
        except Exception as e:
            print(f"   ❌ Error fetching event markets: {e}")
        
        try:
            # 2. SUPPLEMENT: Get standalone markets that might not be in events
            print("🔍 Supplementing with standalone markets...")
            
            markets_api_url = "https://gamma-api.polymarket.com/markets"
            standalone_markets_count = 0
            limit = 100
            offset = 0
            
            # Get standalone markets with proper parameters
            while True:
                try:
                    params = {
                        "order": "id",          # Order by market ID
                        "ascending": "false",   # Get newest markets first
                        "closed": "false",      # Only active markets
                        "limit": str(limit),    # Control response size
                        "offset": str(offset)   # For pagination
                    }
                    
                    print(f"   Fetching standalone markets: offset {offset}, limit {limit}")
                    response = requests.get(markets_api_url, params=params, timeout=30)
                    response.raise_for_status()
                    
                    batch_data = response.json()
                    print(f"   Retrieved {len(batch_data)} markets in this batch")
                    
                    if not batch_data:
                        break
                    
                    # Parse this batch and add as standalone
                    for market in batch_data:
                        # Check if market is active
                        market_active = market.get("active", True)
                        market_closed = market.get("closed", False)
                        
                        if not market_active or market_closed:
                            continue
                        
                        # Parse market outcome prices
                        outcome_prices = market.get("outcomePrices")
                        if not outcome_prices:
                            continue
                        
                        outcome_prices_str = str(outcome_prices)
                        match = re.search(r'\[\"([0-9]+\.[0-9]+)\", \"([0-9]+\.[0-9]+)\"\]', outcome_prices_str)
                        
                        if match:
                            parsed_prices = [float(match.group(1)), float(match.group(2))]
                            
                            # Safely parse volume and liquidity
                            try:
                                volume = market.get("volume", 0)
                                volume = float(volume) if volume is not None else 0
                            except (ValueError, TypeError):
                                volume = 0
                            
                            try:
                                liquidity = market.get("liquidity", 0)
                                liquidity = float(liquidity) if liquidity is not None else 0
                            except (ValueError, TypeError):
                                liquidity = 0
                            
                            parsed_market = {
                                "id": market.get("id"),
                                "slug": market.get("slug"),
                                "outcomePrices": parsed_prices,
                                "question": market.get("question", ""),
                                "description": market.get("description", ""),
                                "endDate": market.get("endDate"),
                                "volume": volume,
                                "liquidity": liquidity,
                                "source": "standalone",
                                "questionID": market.get("questionID"),
                                "conditionId": market.get("conditionId"),
                                "clobTokenIds": _normalize_clob_token_ids(market.get("clobTokenIds", [])),
                                "parent_event_id": None,
                                "parent_event_slug": None,
                                "parent_event_title": None,
                                "parent_event_description": None,
                                "event_tags": []
                            }
                            
                            all_markets.append(parsed_market)
                            standalone_markets_count += 1
                    
                    # If we got fewer than limit, we're on the last page
                    if len(batch_data) < limit:
                        break
                    
                    offset += len(batch_data)
                    
                    # Safety check
                    if offset > 10000:
                        print("   Reached safety limit for standalone markets")
                        break
                        
                except Exception as e:
                    print(f"   Error in standalone batch at offset {offset}: {e}")
                    break
            
            print(f"   Found {standalone_markets_count} standalone markets (supplement)")
            
        except Exception as e:
            print(f"   ❌ Error fetching standalone markets: {e}")
        
        # Remove duplicates by market ID (events take priority over standalone)
        seen_ids = set()
        unique_markets = []
        duplicates_removed = 0
        
        # First pass: Add all event markets (primary source)
        for market in all_markets:
            if market.get('source') == 'event':
                market_id = market.get("id")
                if market_id and market_id not in seen_ids:
                    seen_ids.add(market_id)
                    unique_markets.append(market)
        
        # Second pass: Add standalone markets not already seen
        for market in all_markets:
            if market.get('source') == 'standalone':
                market_id = market.get("id")
                if market_id and market_id not in seen_ids:
                    seen_ids.add(market_id)
                    unique_markets.append(market)
                elif market_id in seen_ids:
                    duplicates_removed += 1
        
        if duplicates_removed > 0:
            print(f"🔄 Removed {duplicates_removed} duplicate markets (events take priority)")
        
        print(f"📊 Total unique active markets: {len(unique_markets)}")
        print(f"   📈 From events: {len([m for m in unique_markets if m.get('source') == 'event'])}")
        print(f"   📈 Standalone: {len([m for m in unique_markets if m.get('source') == 'standalone'])}")
        
        return unique_markets
    
    def save_markets_to_csv(markets):
        """Save all markets to CSV file - overwrites existing file"""
        if not markets:
            print("❌ No markets to save")
            return
        
        # Create data directory - save in polytrading/data not utils/data
        data_dir = os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(__file__))), "data")
        os.makedirs(data_dir, exist_ok=True)
        
        # Use fixed filename (no timestamp) - overwrites existing file
        csv_filename = os.path.join(data_dir, "all_active_markets.csv")
        
        # CSV headers
        headers = [
            'market_id',
            'market_slug',
            'question',
            'description',
            'outcome_price_yes',
            'outcome_price_no',
            'price_spread',
            'volume',
            'liquidity',
            'volume_to_liquidity_ratio',
            'source',
            'questionID',
            'conditionId',
            'clob_token_ids',
            'parent_event_id',
            'parent_event_slug',
            'parent_event_title',
            'parent_event_description',
            'event_tags',
            'end_date'
        ]
        
        # Write to CSV (overwrites existing file)
        with open(csv_filename, 'w', newline='', encoding='utf-8') as csvfile:
            writer = csv.writer(csvfile)
            writer.writerow(headers)
            
            for market in markets:
                prices = market.get('outcomePrices', [0, 0])
                price_yes = prices[0] if len(prices) > 0 else 0
                price_no = prices[1] if len(prices) > 1 else 0
                price_spread = abs(price_yes - price_no) if price_yes and price_no else 0
                
                volume = market.get('volume', 0)
                liquidity = market.get('liquidity', 0)
                
                # Calculate volume to liquidity ratio
                vol_liq_ratio = 0
                try:
                    vol_float = float(volume) if volume else 0
                    liq_float = float(liquidity) if liquidity else 0
                    vol_liq_ratio = vol_float / liq_float if liq_float > 0 else 0
                except (ValueError, TypeError, ZeroDivisionError):
                    vol_liq_ratio = 0
                
                # Format event tags as comma-separated string
                event_tags = market.get('event_tags', [])
                if isinstance(event_tags, list):
                    tags_str = ','.join([str(tag) for tag in event_tags])
                else:
                    tags_str = str(event_tags) if event_tags else ''
                
                # Serialize clobTokenIds as JSON array string to preserve ids exactly
                clob_token_ids = market.get('clobTokenIds', [])
                try:
                    clob_ids_str = json.dumps(clob_token_ids, ensure_ascii=False)
                except Exception:
                    # Fallback to simple string join
                    if isinstance(clob_token_ids, list):
                        clob_ids_str = ','.join([str(token_id) for token_id in clob_token_ids])
                    else:
                        clob_ids_str = str(clob_token_ids) if clob_token_ids else ''
                
                row = [
                    market.get('id', ''),
                    market.get('slug', ''),
                    market.get('question', ''),
                    market.get('description', ''),
                    price_yes,
                    price_no,
                    round(price_spread, 4),
                    volume,
                    liquidity,
                    round(vol_liq_ratio, 4),
                    market.get('source', ''),
                    market.get('questionID', ''),
                    market.get('conditionId', ''),
                    clob_ids_str,
                    market.get('parent_event_id', ''),
                    market.get('parent_event_slug', ''),
                    market.get('parent_event_title', ''),
                    market.get('parent_event_description', ''),
                    tags_str,
                    market.get('endDate', '')
                ]
                writer.writerow(row)
        
        print(f"💾 Saved {len(markets)} markets to: all_active_markets.csv")
        print(f"📁 Location: {os.path.abspath(csv_filename)}")
        return csv_filename
    
    def save_markets_to_json(markets):
        """
        Save all markets to JSON file - clean format for easy data processing
        
        Purpose: Save market data in JSON format which preserves data types and structure
        better than CSV. Useful for data analysis and integration with other tools.
        
        Args:
            markets: List of market dictionaries to save
            
        Returns:
            str: Path to the saved JSON file
        """
        if not markets:
            print("❌ No markets to save")
            return None
        
        # Create data directory - save in polytrading/data not utils/data
        data_dir = os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(__file__))), "data")
        os.makedirs(data_dir, exist_ok=True)
        
        # Use fixed filename (no timestamp) - overwrites existing file
        json_filename = os.path.join(data_dir, "all_active_markets.json")
        
        # Prepare data for JSON serialization
        json_data = {
            "metadata": {
                "total_markets": len(markets),
                "extraction_timestamp": datetime.now().isoformat(),
                "source": "Gamma API (events + standalone markets)",
                "description": "Complete dataset of active Polymarket markets with full metadata"
            },
            "markets": []
        }
        
        # Process each market for JSON format
        for market in markets:
            # Calculate additional metrics
            prices = market.get('outcomePrices', [0, 0])
            price_yes = prices[0] if len(prices) > 0 else 0
            price_no = prices[1] if len(prices) > 1 else 0
            price_spread = abs(price_yes - price_no) if price_yes and price_no else 0
            
            volume = market.get('volume', 0)
            liquidity = market.get('liquidity', 0)
            
            # Calculate volume to liquidity ratio
            vol_liq_ratio = 0
            try:
                vol_float = float(volume) if volume else 0
                liq_float = float(liquidity) if liquidity else 0
                vol_liq_ratio = vol_float / liq_float if liq_float > 0 else 0
            except (ValueError, TypeError, ZeroDivisionError):
                vol_liq_ratio = 0
            
            # Create enhanced market record
            market_record = {
                "id": market.get('id', ''),
                "slug": market.get('slug', ''),
                "question": market.get('question', ''),
                "description": market.get('description', ''),
                "prices": {
                    "yes": price_yes,
                    "no": price_no,
                    "spread": round(price_spread, 4)
                },
                "volume": volume,
                "liquidity": liquidity,
                "metrics": {
                    "volume_to_liquidity_ratio": round(vol_liq_ratio, 4)
                },
                "trading": {
                    "clob_token_ids": market.get('clobTokenIds', [])
                },
                "metadata": {
                    "source": market.get('source', ''),
                    "question_id": market.get('questionID', ''),
                    "condition_id": market.get('conditionId', ''),
                    "end_date": market.get('endDate', '')
                },
                "event_context": {
                    "parent_event_id": market.get('parent_event_id'),
                    "parent_event_slug": market.get('parent_event_slug'),
                    "parent_event_title": market.get('parent_event_title'),
                    "parent_event_description": market.get('parent_event_description'),
                    "event_tags": market.get('event_tags', [])
                } if market.get('parent_event_id') else None
            }
            
            json_data["markets"].append(market_record)
        
        # Write to JSON file with proper formatting
        try:
            with open(json_filename, 'w', encoding='utf-8') as jsonfile:
                json.dump(json_data, jsonfile, indent=2, ensure_ascii=False)
            
            print(f"💾 Saved {len(markets)} markets to: all_active_markets.json")
            print(f"📁 Location: {os.path.abspath(json_filename)}")
            print(f"📊 JSON includes metadata and structured format for easy analysis")
            return json_filename
            
        except Exception as e:
            print(f"❌ Error saving JSON file: {e}")
            return None
    
    # Main execution
    print("🚀 Extracting ALL Active Markets from Polymarket")
    print("=" * 60)
    
    # Get all markets
    all_markets = get_all_active_markets()
    
    if all_markets:
        # Save to both CSV and JSON formats
        csv_file = save_markets_to_csv(all_markets)
        json_file = save_markets_to_json(all_markets)
        
        # Show breakdown
        standalone_count = sum(1 for m in all_markets if m.get('source') == 'standalone')
        event_count = sum(1 for m in all_markets if m.get('source') == 'event')
        
        # Volume analysis
        volumes = []
        for market in all_markets:
            try:
                vol = float(market.get('volume', 0))
                volumes.append(vol)
            except (ValueError, TypeError):
                volumes.append(0)
        
        high_volume_count = sum(1 for v in volumes if v > 1000)
        total_volume = sum(volumes)
        avg_volume = total_volume / len(volumes) if volumes else 0
        
        print(f"\n📊 Market Summary:")
        print(f"   🔸 Standalone markets: {standalone_count}")
        print(f"   🔸 Event markets: {event_count}")
        print(f"   🔸 Total active markets: {len(all_markets)}")
        print(f"   💰 High volume markets (>$1K): {high_volume_count}")
        print(f"   📈 Total volume: ${total_volume:,.2f}")
        print(f"   📊 Average volume: ${avg_volume:.2f}")
        
        # Show event grouping analysis
        event_groups = {}
        for market in all_markets:
            if market.get('source') == 'event':
                event_id = market.get('parent_event_id')
                if event_id:
                    if event_id not in event_groups:
                        event_groups[event_id] = {
                            'title': market.get('parent_event_title', 'Unknown Event'),
                            'count': 0
                        }
                    event_groups[event_id]['count'] += 1
        
        print(f"\n🎯 Event Grouping Analysis:")
        print(f"   📅 Total events with markets: {len(event_groups)}")
        
        # Show top events by market count
        sorted_events = sorted(event_groups.items(), key=lambda x: x[1]['count'], reverse=True)
        print(f"   🏆 Top events by market count:")
        for i, (event_id, info) in enumerate(sorted_events[:5]):
            title = info['title'][:40] + "..." if len(info['title']) > 40 else info['title']
            print(f"      {i+1}. {title} ({info['count']} markets)")
        
        # Show example markets with grouping info
        print(f"\n📈 Example Markets with Grouping:")
        for i, market in enumerate(all_markets[:3]):
            question = market.get('question', 'N/A')
            if len(question) > 40:
                question = question[:40] + "..."
            prices = market.get('outcomePrices', [])
            source = market.get('source', '')
            event_title = market.get('parent_event_title', '')
            
            print(f"   {i+1}. {question} ({source})")
            print(f"      Prices: {prices} | Volume: ${market.get('volume', 0)}")
            if event_title:
                print(f"      Event: {event_title[:30]}..." if len(event_title) > 30 else f"      Event: {event_title}")
            else:
                print(f"      Event: (standalone market)")
        
        print(f"\n✅ All active markets extracted successfully!")
    else:
        print("❌ No markets found")
    
    print("=" * 60)