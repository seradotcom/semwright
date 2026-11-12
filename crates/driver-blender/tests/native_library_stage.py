"""Pure Windows/Linux regression for Blender's private new-only library stage.

Blender writes to <name>.blend@ before replacing <name>.blend; unlike the old
mkstemp path, the target must not exist when bpy.data.libraries.write starts.
The test evaluates the actual pure helper extracted from both pinned Python
sources without importing Blender or invoking it.
"""
from __future__ import annotations
import ast
import os
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
SOURCES = (
    ROOT / "crates/driver-blender/src/authoring_native.py",
    ROOT / "crates/driver-host/src/windows_blender52_runtime/authoring_native.py",
)


def load_stage_helper(path: Path):
    tree=ast.parse(path.read_text(encoding="utf-8"))
    functions=[node for node in tree.body
               if isinstance(node,ast.FunctionDef)
               and node.name=="_private_library_stage"]
    if len(functions)!=1:
        raise AssertionError("Expected one source-bound private stage helper")
    function=functions[0]
    module=ast.Module(body=[function],type_ignores=[])
    built=compile(module,str(path),"exec")
    def check(condition,message,code="InvalidArgument"):
        if not condition:
            raise RuntimeError(code+": "+message)
    env={"os":os,"tempfile":tempfile,"check":check}
    exec(built,env)
    return env["_private_library_stage"]


class NativeBlenderPrivateLibraryStaging(unittest.TestCase):
    def setUp(self):
        temp=tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.workspace=Path(temp.name).resolve()

    def test_private_stage_never_precreates_blender_target(self):
        for path in SOURCES:
            with self.subTest(source=path.name,relative=str(path.parent)):
                helper=load_stage_helper(path)
                destination=self.workspace/"example.blend"
                root, target=helper(str(destination))
                root, target=Path(root),Path(target)
                self.assertTrue(root.is_dir())
                self.assertEqual(root.parent,self.workspace)
                self.assertEqual(target.parent,root)
                self.assertEqual(target.name,"library.blend")
                self.assertFalse(target.exists())
                self.assertFalse(destination.exists())
                # Blender's @ intermediate must be able to rename to a
                # previously *nonexistent* .blend path.
                intermediate=Path(str(target)+"@")
                intermediate.write_bytes(b"BLENDER-v5-native-file")
                os.rename(intermediate,target)
                self.assertEqual(target.read_bytes(),b"BLENDER-v5-native-file")
                os.link(target,destination)
                self.assertEqual(destination.read_bytes(),target.read_bytes())
                target.unlink()
                root.rmdir()
                destination.unlink()

    def test_exclusive_stages_and_no_clobber_publishing(self):
        for path in SOURCES:
            with self.subTest(source=str(path)):
                helper=load_stage_helper(path)
                destination=self.workspace/"existing.blend"
                destination.write_bytes(b"owner existing data")
                created=[]
                for _ in range(3):
                    root,target=map(Path,helper(str(destination)))
                    self.assertFalse(target.exists())
                    target.write_bytes(b"new staged data")
                    with self.assertRaises(FileExistsError):
                        os.link(target,destination)
                    self.assertEqual(destination.read_bytes(),b"owner existing data")
                    created.append((root,target))
                self.assertEqual(len({str(root) for root,_ in created}),3)
                for root,target in created:
                    target.unlink()
                    root.rmdir()
                destination.unlink()



def load_persist_method(path: Path):
    tree=ast.parse(path.read_text(encoding="utf-8"))
    stages=[node for node in tree.body
            if isinstance(node,ast.FunctionDef)
            and node.name=="_private_library_stage"]
    methods=[node for node in ast.walk(tree)
             if isinstance(node,ast.FunctionDef) and node.name=="persist"]
    assert len(stages)==len(methods)==1
    compiled=compile(ast.Module(body=[stages[0],methods[0]],type_ignores=[]),str(path),"exec")
    import hashlib
    import stat
    from types import SimpleNamespace

    class CommandError(Exception):
        def __init__(self, code, message):
            super().__init__(message)
            self.code=code

    def check(condition,message,code="InvalidArgument"):
        if not condition:
            raise CommandError(code,message)

    environment={
        "os":os,"stat":stat,"tempfile":tempfile,"hashlib":hashlib,
        "Path":Path,"CommandError":CommandError,"check":check,
    }
    exec(compiled,environment)
    return environment["persist"],CommandError


class NativePersistRealBodyTests(unittest.TestCase):
    def setUp(self):
        temp=tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.workspace=Path(temp.name).resolve()

    def make_runtime(self, *, fail_after_temporary: bool=False):
        from types import SimpleNamespace

        class Workspace:
            def __init__(self, root):
                self.root=root

            def path(self, relative, suffix):
                assert isinstance(relative,str) and relative.endswith(suffix)
                assert relative in {"positive.blend", "existing.blend", "failing.blend"}
                return str(self.root / relative)

        class Libraries:
            calls=0

            def write(self, path, blocks, *, path_remap, fake_user, compress):
                self.calls+=1
                assert not os.path.lexists(path), "Blender writer received precreated file"
                assert path_remap=="NONE" and not fake_user and compress
                assert len(blocks)==1
                intermediate=Path(path+"@")
                intermediate.write_bytes(b"BLENDER-fake-library-test")
                if fail_after_temporary:
                    raise OSError("simulated native failure before Blender rename")
                os.rename(intermediate,path)

        libraries=Libraries()
        class Collection:
            name="SW_managed_test"

        collection=Collection()
        runtime=SimpleNamespace(
            commands=SimpleNamespace(workspace=Workspace(self.workspace)),
            bpy=SimpleNamespace(data=SimpleNamespace(libraries=libraries)),
            _closed=lambda island:(collection,[SimpleNamespace(name="Triangle")]),
            snapshot=lambda island:{"drift":False,"fingerprint":"a"*64},
        )
        return runtime,libraries

    def test_actual_persist_writes_and_publishes_new_only_file(self):
        import hashlib
        for path in SOURCES:
            with self.subTest(source=str(path)):
                persist,_=load_persist_method(path)
                runtime,libraries=self.make_runtime()
                receipt=persist(runtime,"managed-test","positive.blend")
                output=self.workspace/"positive.blend"
                self.assertEqual(libraries.calls,1)
                self.assertEqual(output.read_bytes(),b"BLENDER-fake-library-test")
                self.assertEqual(receipt["sha256"],hashlib.sha256(output.read_bytes()).hexdigest())
                self.assertEqual(receipt["source_fingerprint"],"a"*64)
                self.assertEqual(receipt["path"],"positive.blend")
                self.assertFalse(list(self.workspace.glob(".semwright-authoring-*")))
                output.unlink()

    def test_existing_destination_denied_before_native_writer(self):
        for path in SOURCES:
            with self.subTest(source=str(path)):
                persist,CommandError=load_persist_method(path)
                output=self.workspace/"existing.blend"
                output.write_bytes(b"do-not-overwrite-owner")
                runtime,libraries=self.make_runtime()
                with self.assertRaises(CommandError) as error:
                    persist(runtime,"managed-test","existing.blend")
                self.assertEqual(error.exception.code,"Conflict")
                self.assertEqual(libraries.calls,0)
                self.assertEqual(output.read_bytes(),b"do-not-overwrite-owner")
                self.assertFalse(list(self.workspace.glob(".semwright-authoring-*")))
                output.unlink()

    def test_native_failure_keeps_private_unpublished_stage_for_reconciliation(self):
        for path in SOURCES:
            with self.subTest(source=str(path)):
                persist,_=load_persist_method(path)
                runtime,libraries=self.make_runtime(fail_after_temporary=True)
                with self.assertRaises(OSError):
                    persist(runtime,"managed-test","failing.blend")
                self.assertEqual(libraries.calls,1)
                self.assertFalse((self.workspace/"failing.blend").exists())
                staged=list(self.workspace.glob(".semwright-authoring-*"))
                self.assertEqual(len(staged),1)
                self.assertTrue((staged[0]/"library.blend@").exists())
                self.assertFalse((staged[0]/"library.blend").exists())
                (staged[0]/"library.blend@").unlink()
                staged[0].rmdir()


if __name__=="__main__":
    unittest.main()
