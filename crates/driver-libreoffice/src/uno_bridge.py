import json
import os
import sys
import time

import uno
from com.sun.star.beans import PropertyValue

pipe_name = sys.argv[1]
operation = sys.argv[2]
args = json.loads(sys.argv[3])


def reply(data):
    print(json.dumps({"ok": True, "data": data}, separators=(",", ":")))


def fail(code):
    print(json.dumps({"ok": False, "code": code}, separators=(",", ":")))


def prop(name, value):
    item = PropertyValue()
    item.Name = name
    item.Value = value
    return item


def connect():
    local = uno.getComponentContext()
    resolver = local.ServiceManager.createInstanceWithContext(
        "com.sun.star.bridge.UnoUrlResolver", local
    )
    for _ in range(40):
        try:
            return resolver.resolve(
                f"uno:pipe,name={pipe_name};urp;StarOffice.ComponentContext"
            )
        except Exception:
            time.sleep(0.025)
    raise RuntimeError("UNO connection unavailable")


def product_version():
    try:
        with open(
            "/usr/lib/libreoffice/program/versionrc", "r", encoding="utf-8"
        ) as handle:
            for line in handle:
                if line.startswith("ProductVersion="):
                    return line.split("=", 1)[1].strip()
    except OSError:
        pass
    return "unknown"


def file_url(path):
    return uno.systemPathToFileUrl(path)


def load(desktop, path, read_only=True):
    return desktop.loadComponentFromURL(
        file_url(path),
        "_blank",
        0,
        (prop("Hidden", True), prop("ReadOnly", read_only)),
    )


ctx = connect()
smgr = ctx.ServiceManager
desktop = smgr.createInstanceWithContext("com.sun.star.frame.Desktop", ctx)

try:
    if operation == "status":
        reply(
            {
                "connected": True,
                "product": "LibreOffice",
                "version": product_version(),
            }
        )

    elif operation == "writer_create":
        path = args["path"]
        if os.path.exists(path):
            fail("Conflict")
        else:
            doc = desktop.loadComponentFromURL(
                "private:factory/swriter",
                "_blank",
                0,
                (prop("Hidden", True),),
            )
            try:
                doc.Text.String = args["text"]
                doc.storeAsURL(file_url(path), (prop("FilterName", "writer8"),))
            finally:
                doc.close(True)
            reply({"created": True})

    elif operation == "writer_read":
        path = args["path"]
        if not os.path.isfile(path):
            fail("NotFound")
        else:
            doc = load(desktop, path, True)
            try:
                reply({"text": doc.Text.String})
            finally:
                doc.close(True)

    elif operation == "calc_create":
        path = args["path"]
        if os.path.exists(path):
            fail("Conflict")
        else:
            doc = desktop.loadComponentFromURL(
                "private:factory/scalc",
                "_blank",
                0,
                (prop("Hidden", True),),
            )
            try:
                sheet = doc.Sheets.getByIndex(0)
                for address, value in args["cells"].items():
                    cell = sheet.getCellRangeByName(address)
                    if isinstance(value, (int, float)) and not isinstance(
                        value, bool
                    ):
                        cell.Value = float(value)
                    else:
                        cell.String = str(value)
                doc.storeAsURL(file_url(path), (prop("FilterName", "calc8"),))
            finally:
                doc.close(True)
            reply({"created": True, "cells_written": len(args["cells"])})

    elif operation == "calc_get":
        path = args["path"]
        if not os.path.isfile(path):
            fail("NotFound")
        else:
            doc = load(desktop, path, True)
            try:
                cell = doc.Sheets.getByIndex(0).getCellRangeByName(args["cell"])
                kind = getattr(cell.Type, "value", "EMPTY")
                if kind == "EMPTY":
                    normalized = "empty"
                    value = None
                elif kind == "VALUE":
                    normalized = "number"
                    value = cell.Value
                elif kind == "FORMULA":
                    normalized = "formula"
                    value = cell.Formula
                else:
                    normalized = "text"
                    value = cell.String
                reply({"kind": normalized, "value": value})
            finally:
                doc.close(True)

    elif operation == "calc_set":
        path = args["path"]
        if not os.path.isfile(path):
            fail("NotFound")
        else:
            doc = load(desktop, path, False)
            try:
                cell = doc.Sheets.getByIndex(0).getCellRangeByName(args["cell"])
                value = args["value"]
                if isinstance(value, (int, float)) and not isinstance(
                    value, bool
                ):
                    cell.Value = float(value)
                else:
                    cell.String = str(value)
                doc.store()
            finally:
                doc.close(True)
            reply({"updated": True})

    elif operation == "export_pdf":
        source = args["path"]
        output = args["output"]
        if not os.path.isfile(source):
            fail("NotFound")
        elif os.path.exists(output):
            fail("Conflict")
        else:
            doc = load(desktop, source, True)
            try:
                services = tuple(doc.getSupportedServiceNames())
                if "com.sun.star.sheet.SpreadsheetDocument" in services:
                    filter_name = "calc_pdf_Export"
                elif "com.sun.star.text.TextDocument" in services:
                    filter_name = "writer_pdf_Export"
                else:
                    raise RuntimeError("Unsupported document type")
                doc.storeToURL(
                    file_url(output),
                    (prop("FilterName", filter_name),),
                )
            finally:
                doc.close(True)
            reply({"exported": True})

    else:
        fail("Unsupported")
except KeyError:
    fail("InvalidArgument")
except Exception:
    fail("BackendFailed")
