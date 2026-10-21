from __future__ import annotations

import contextvars
import logging
from typing import Optional

_run_id_var: contextvars.ContextVar[str] = contextvars.ContextVar("run_id", default="-")
_record_factory_installed = False


def _install_log_record_factory() -> None:
    global _record_factory_installed
    if _record_factory_installed:
        return
    old_factory = logging.getLogRecordFactory()

    def record_factory(*args, **kwargs):
        record = old_factory(*args, **kwargs)
        record.run_id = _run_id_var.get()
        return record

    logging.setLogRecordFactory(record_factory)
    _record_factory_installed = True


_install_log_record_factory()


def set_run_id(run_id: Optional[str]) -> contextvars.Token:
    rid = (run_id or "").strip() or "-"
    return _run_id_var.set(rid)


def reset_run_id(token: contextvars.Token) -> None:
    _run_id_var.reset(token)


def get_run_id() -> str:
    return _run_id_var.get()
