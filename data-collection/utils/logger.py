"""
PMTerminal.utils.logger

Simple logging setup for PMTerminal. Exposes:
- setup_command_logger(...)  -- preferred name used by CommandsManager
- setup_logger(...)          -- compatibility wrapper for imports expecting setup_logger

Logs are written to PMTerminal/logs/commands.log (rotating).
"""
from __future__ import annotations

import logging
import os
from logging.handlers import RotatingFileHandler
from typing import Optional

# Directory for logs: PMTerminal/logs
LOG_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "logs"))
LOG_FILE = "commands.log"
LOG_PATH = os.path.abspath(os.path.join(LOG_DIR, LOG_FILE))


def _ensure_log_dir():
    os.makedirs(LOG_DIR, exist_ok=True)


def setup_command_logger(level: int = logging.DEBUG, *, log_file: Optional[str] = None) -> logging.Logger:
    """
    Configure and return a logger that writes to PMTerminal/logs/commands.log (or provided log_file).

    Safe to call multiple times: it will not attach duplicate file handlers for the same file.
    """
    _ensure_log_dir()
    path = LOG_PATH if log_file is None else os.path.abspath(os.path.join(LOG_DIR, log_file))

    logger = logging.getLogger("PMTerminal.CommandsManager")
    logger.setLevel(level)

    # avoid duplicate RotatingFileHandler instances for the same file
    for h in list(logger.handlers):
        if isinstance(h, RotatingFileHandler) and os.path.abspath(h.baseFilename) == path:
            # handler already exists for this file
            return logger

    fh = RotatingFileHandler(path, maxBytes=2 * 1024 * 1024, backupCount=3, encoding="utf-8")
    fmt = logging.Formatter("%(asctime)s [%(levelname)s] %(name)s:%(lineno)d | %(message)s", "%Y-%m-%d %H:%M:%S")
    fh.setFormatter(fmt)
    fh.setLevel(level)
    logger.addHandler(fh)

    # Add a console handler for development convenience if none exists
    if not any(isinstance(h, logging.StreamHandler) for h in logger.handlers):
        ch = logging.StreamHandler()
        ch.setLevel(level)
        ch.setFormatter(logging.Formatter("%(asctime)s %(levelname)s %(message)s", "%H:%M:%S"))
        logger.addHandler(ch)

    return logger


def setup_logger(level: int = logging.DEBUG, *, log_file: Optional[str] = None) -> logging.Logger:
    """
    Compatibility wrapper used by older code that imports `setup_logger`.

    Delegates to setup_command_logger so both import names behave the same.
    """
    return setup_command_logger(level, log_file=log_file)


# Exported API
__all__ = ["setup_command_logger", "setup_logger"]