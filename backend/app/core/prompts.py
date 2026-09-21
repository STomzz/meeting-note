"""提示词加载（Jinja2）。提示词外置在 ``app/prompts/``，改文案不用改代码。"""

from __future__ import annotations

from pathlib import Path

import jinja2


def create_prompts_env(prompts_dir: Path) -> jinja2.Environment:
    if not prompts_dir.is_dir():
        raise RuntimeError(f"提示词目录不存在：{prompts_dir}")

    env = jinja2.Environment(
        loader=jinja2.FileSystemLoader(str(prompts_dir)),
        autoescape=False,
        undefined=jinja2.StrictUndefined,
        keep_trailing_newline=True,
    )
    env.globals["now"] = None  # 预留：模板里需要时间时可注入
    return env
