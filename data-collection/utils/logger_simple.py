"""
Simple logger setup used by CommandsManager and other components.

Creates PMTerminal/logs/commands.log (rotating) and returns a logger instance.
"""
from __future__ import annotations

import logging
import os
from logging.handlers import RotatingFileHandler
from typing import Optional

LOG_DIR = os.path.join(os.path.dirname(__file__), "..", "logs")
LOG_FILE = "commands.log"
LOG_PATH = os.path.abspath(os.path.join(LOG_DIR, LOG_FILE))


def setup_command_logger(level: int = logging.DEBUG) -> logging.Logger:
    """
    Ensure logs directory exists and return a logger writing to PMTerminal/logs/commands.log.

    This is safe to call multiple times; it will not attach duplicate file handlers.
    """
    os.makedirs(LOG_DIR, exist_ok=True)

    logger = logging.getLogger("PMTerminal.CommandsManager")
    logger.setLevel(level)

    # avoid duplicate RotatingFileHandler instances for the same file
    existing = []
    for h in logger.handlers:
        if isinstance(h, RotatingFileHandler) and os.path.abspath(h.baseFilename) == LOG_PATH:
            existing.append(h)
    if not existing:
        fh = RotatingFileHandler(LOG_PATH, maxBytes=2 * 1024 * 1024, backupCount=3, encoding="utf-8")
        fmt = logging.Formatter("%(asctime)s [%(levelname)s] %(name)s:%(lineno)d | %(message)s", "%Y-%m-%d %H:%M:%S")
        fh.setFormatter(fmt)
        fh.setLevel(level)
        logger.addHandler(fh)

    # ensure at least one console handler for development (optional)
    if not any(isinstance(h, logging.StreamHandler) for h in logger.handlers):
        ch = logging.StreamHandler()
        ch.setLevel(level)
        ch.setFormatter(logging.Formatter("%(asctime)s %(levelname)s %(message)s", "%H:%M:%S"))
        logger.addHandler(ch)

    return logger