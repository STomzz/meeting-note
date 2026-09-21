"""日志初始化。"""

from __future__ import annotations

import logging
import sys


def setup_logging(level: str = "INFO") -> None:
    """配置根日志（幂等）。"""

    root = logging.getLogger()
    if root.handlers:
        for handler in list(root.handlers):
            root.removeHandler(handler)

    handler = logging.StreamHandler(sys.stdout)
    handler.setFormatter(
        logging.Formatter(
            fmt="%(asctime)s %(levelname)-7s [%(name)s] %(message)s",
            datefmt="%Y-%m-%d %H:%M:%S",
        )
    )
    root.addHandler(handler)
    root.setLevel(level.upper())

    # 降低噪音
    logging.getLogger("httpx").setLevel(logging.WARNING)
    logging.getLogger("httpcore").setLevel(logging.WARNING)
