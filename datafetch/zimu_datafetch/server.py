"""uvicorn 入口。

    cd datafetch
    uv run uvicorn zimu_datafetch.server:app --host 127.0.0.1 --port 8300
"""

from .app import create_app

app = create_app()
