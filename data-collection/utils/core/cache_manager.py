"""
Cache Manager Utility
Provides TTL-based caching for JSON data files
"""
import json
from pathlib import Path
from datetime import datetime, timedelta
from typing import Optional, Dict, Any
import logging

logger = logging.getLogger(__name__)

# Cache directory path
CACHE_DIR = Path(__file__).parent.parent.parent / "data"


def ensure_cache_dir() -> None:
    """Ensure cache directory exists"""
    CACHE_DIR.mkdir(parents=True, exist_ok=True)


def get_cache_path(filename: str) -> Path:
    """Get full path to cache file"""
    return CACHE_DIR / filename


def is_cache_valid(filename: str, ttl_seconds: int = 300) -> bool:
    """
    Check if cache file exists and is within TTL
    
    Args:
        filename: Cache file name (e.g., 'live_football_matches.json')
        ttl_seconds: Time-to-live in seconds (default: 5 minutes)
    
    Returns:
        True if cache is valid, False otherwise
    """
    cache_path = get_cache_path(filename)
    
    if not cache_path.exists():
        return False
    
    try:
        with open(cache_path, 'r', encoding='utf-8') as f:
            data = json.load(f)
        
        # Check if metadata exists and has timestamp
        metadata = data.get('metadata', {})
        timestamp_str = metadata.get('extraction_timestamp')
        
        if not timestamp_str:
            logger.warning(f"Cache file {filename} missing timestamp")
            return False
        
        # Parse timestamp
        cache_time = datetime.fromisoformat(timestamp_str)
        current_time = datetime.now()
        
        # Check if within TTL
        age = (current_time - cache_time).total_seconds()
        is_valid = age <= ttl_seconds
        
        if is_valid:
            logger.debug(f"Cache {filename} valid (age: {age:.1f}s / {ttl_seconds}s)")
        else:
            logger.debug(f"Cache {filename} expired (age: {age:.1f}s / {ttl_seconds}s)")
        
        return is_valid
        
    except Exception as e:
        logger.error(f"Error checking cache validity for {filename}: {e}")
        return False


def read_cache(filename: str, ttl_seconds: int = 300) -> Optional[Dict[str, Any]]:
    """
    Read cache file if valid
    
    Args:
        filename: Cache file name
        ttl_seconds: Time-to-live in seconds
    
    Returns:
        Cache data dict if valid, None otherwise
    """
    cache_path = get_cache_path(filename)
    
    if not cache_path.exists():
        logger.debug(f"Cache file {filename} does not exist")
        return None
    
    try:
        with open(cache_path, 'r', encoding='utf-8') as f:
            data = json.load(f)
        
        # Check validity
        if not is_cache_valid(filename, ttl_seconds):
            logger.info(f"Cache {filename} expired, returning None")
            return None
        
        logger.info(f"✓ Loaded valid cache from {filename}")
        return data
        
    except Exception as e:
        logger.error(f"Error reading cache {filename}: {e}")
        return None


def read_cache_stale_ok(filename: str) -> Optional[Dict[str, Any]]:
    """
    Read cache file regardless of TTL (stale-while-revalidate pattern)
    Useful as fallback when API is down
    
    Args:
        filename: Cache file name
    
    Returns:
        Cache data dict if exists, None otherwise
    """
    cache_path = get_cache_path(filename)
    
    if not cache_path.exists():
        return None
    
    try:
        with open(cache_path, 'r', encoding='utf-8') as f:
            data = json.load(f)
        
        logger.info(f"⚠ Loaded stale cache from {filename} (fallback mode)")
        return data
        
    except Exception as e:
        logger.error(f"Error reading stale cache {filename}: {e}")
        return None


def write_cache(
    filename: str, 
    data: Any, 
    metadata: Optional[Dict[str, Any]] = None,
    ttl_seconds: int = 300
) -> bool:
    """
    Write data to cache file with metadata
    
    Args:
        filename: Cache file name
        data: Data to cache (will be stored under 'data' key or directly if dict with metadata)
        metadata: Optional custom metadata (merged with default)
        ttl_seconds: TTL for reference in metadata
    
    Returns:
        True if successful, False otherwise
    """
    ensure_cache_dir()
    cache_path = get_cache_path(filename)
    
    try:
        # Build default metadata
        default_metadata = {
            "extraction_timestamp": datetime.now().isoformat(),
            "ttl_seconds": ttl_seconds,
            "cache_file": filename
        }
        
        # Merge with custom metadata
        if metadata:
            default_metadata.update(metadata)
        
        # Structure cache data
        # If data is already a dict with metadata key, use as-is
        if isinstance(data, dict) and 'metadata' in data:
            cache_data = data
            # Update metadata
            cache_data['metadata'].update(default_metadata)
        else:
            # Wrap data with metadata
            cache_data = {
                "metadata": default_metadata,
                "data": data
            }
        
        # Write to file (pretty-printed for debugging)
        with open(cache_path, 'w', encoding='utf-8') as f:
            json.dump(cache_data, f, indent=2, ensure_ascii=False)
        
        logger.info(f"✓ Wrote cache to {filename}")
        return True
        
    except Exception as e:
        logger.error(f"Error writing cache {filename}: {e}")
        return False


def clear_cache(filename: str) -> bool:
    """
    Remove cache file
    
    Args:
        filename: Cache file name
    
    Returns:
        True if removed, False otherwise
    """
    cache_path = get_cache_path(filename)
    
    try:
        if cache_path.exists():
            cache_path.unlink()
            logger.info(f"✓ Cleared cache {filename}")
            return True
        else:
            logger.debug(f"Cache {filename} doesn't exist, nothing to clear")
            return False
            
    except Exception as e:
        logger.error(f"Error clearing cache {filename}: {e}")
        return False


def get_cache_age(filename: str) -> Optional[float]:
    """
    Get cache age in seconds
    
    Args:
        filename: Cache file name
    
    Returns:
        Age in seconds, or None if cache doesn't exist
    """
    cache_path = get_cache_path(filename)
    
    if not cache_path.exists():
        return None
    
    try:
        with open(cache_path, 'r', encoding='utf-8') as f:
            data = json.load(f)
        
        metadata = data.get('metadata', {})
        timestamp_str = metadata.get('extraction_timestamp')
        
        if not timestamp_str:
            return None
        
        cache_time = datetime.fromisoformat(timestamp_str)
        current_time = datetime.now()
        
        age = (current_time - cache_time).total_seconds()
        return age
        
    except Exception as e:
        logger.error(f"Error getting cache age for {filename}: {e}")
        return None
