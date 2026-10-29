"""Lifecycle preservation guards; fake native writer tests are not Blender certification."""
import hashlib
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace as N
import unittest
from unittest.mock import Mock
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "adapters/blender"))
from semwright_blender.commands import Commands, checkpoint_preflight
from semwright_blender.validation import CommandError

def ident(**kw):
    values = dict(users=1, use_fake_user=False, is_embedded_data=False,
                  library=None, override_library=None, animation_data=None)
    values.update(kw)
    return N(**values)

def fixture():
    scene = ident(sequence_editor=None, use_nodes=False, rigidbody_world=None)
    obj = ident(type="MESH", modifiers=[], constraints=[], particle_systems=[],
                rigid_body=None, rigid_body_constraint=None, is_instancer=False)
    groups = dict(scenes=[scene], objects=[obj], meshes=[ident()], materials=[],
                  worlds=[], lights=[], images=[])
    data = N(**groups, filepath="original.blend",
             bl_rna=N(properties=[N(type="COLLECTION",identifier=k) for k in groups]))
    bpy = N(data=data, context=N(mode="OBJECT",scene=scene,preferences=N(filepaths=N(save_version=2))),
            app=N(is_job_running=lambda _:False),
            ops=N(wm=N(save_as_mainfile=Mock(),open_mainfile=Mock(return_value={"FINISHED"}))))
    def save(**kwargs):
        Path(kwargs["filepath"]).write_bytes(b"BLENDER-v300" + b"test-only-not-a-native-blend")
        return {"FINISHED"}
    bpy.ops.wm.save_as_mainfile.side_effect=save
    return bpy

class CheckpointTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name).resolve()
        self.bpy=fixture()
        self.commands=Commands(self.bpy,str(self.root))

    def reject_before_save(self,code):
        with self.assertRaises(CommandError) as error:
            self.commands("blender.file.checkpoint",{"path":"recovery.blend"})
        self.assertEqual(error.exception.code,code)
        self.bpy.ops.wm.save_as_mainfile.assert_not_called()
        self.assertFalse((self.root/"recovery.blend").exists())

    def test_mode_rejected_before_save(self):
        self.bpy.context.mode="EDIT_MESH"
        self.reject_before_save("Conflict")

    def test_dirty_image_and_unknown_category_rejected_before_save(self):
        self.bpy.data.images.append(ident(is_dirty=True))
        self.reject_before_save("Unsupported")
        self.bpy.data.images.clear()
        self.bpy.data.new_future_cache=[ident()]
        self.bpy.data.bl_rna.properties.append(N(type="COLLECTION",identifier="new_future_cache"))
        self.reject_before_save("Unsupported")

    def test_modifier_cache_rejected_before_save(self):
        self.bpy.data.objects[0].modifiers.append(N(type="CLOTH"))
        self.reject_before_save("Unsupported")

    def test_orphan_mesh_rejected_before_save(self):
        self.bpy.data.meshes[0].users=0
        self.reject_before_save("Unsupported")

    def test_existing_file_preserved(self):
        target=self.root/"recovery.blend"
        target.write_bytes(b"existing")
        with self.assertRaises(CommandError) as error:
            self.commands("blender.file.checkpoint",{"path":"recovery.blend"})
        self.assertEqual(error.exception.code,"Conflict")
        self.assertEqual(target.read_bytes(),b"existing")
        self.bpy.ops.wm.save_as_mainfile.assert_not_called()

    def test_new_copy_verified_and_preferences_restored(self):
        receipt=self.commands("blender.file.checkpoint",{"path":"recovery.blend"})
        target=self.root/"recovery.blend"
        self.assertEqual(receipt["sha256"],hashlib.sha256(target.read_bytes()).hexdigest())
        self.assertEqual(receipt["bytes"],target.stat().st_size)
        self.assertEqual(target.stat().st_nlink,1)
        self.assertEqual(self.bpy.data.filepath,"original.blend")
        self.assertEqual(self.bpy.context.preferences.filepaths.save_version,2)
        self.assertFalse(list(self.root.glob(".semwright-checkpoint-*")))
        self.assertFalse(receipt["undo_history_preserved"])

    def test_failed_native_save_is_typed_and_preserves_session(self):
        self.bpy.ops.wm.save_as_mainfile.side_effect=OSError("bounded injected write failure")
        with self.assertRaises(CommandError) as error:
            self.commands("blender.file.checkpoint",{"path":"recovery.blend"})
        self.assertEqual(error.exception.code,"BackendFailed")
        self.assertEqual(self.bpy.context.preferences.filepaths.save_version,2)
        self.assertEqual(self.bpy.data.filepath,"original.blend")
        self.assertFalse(list(self.root.iterdir()))

    def test_initial_open_hash_failure_consumes_one_shot_without_loading(self):
        (self.root/"input.blend").write_bytes(b"BLENDER-v300" + b"fixture")
        with self.assertRaises(CommandError) as error:
            self.commands("blender.file.open_initial",{"path":"input.blend","expected_sha256":"0"*64})
        self.assertEqual(error.exception.code,"Conflict")
        self.bpy.ops.wm.open_mainfile.assert_not_called()
        with self.assertRaises(CommandError):
            self.commands("blender.file.open_initial",{"path":"input.blend","expected_sha256":"0"*64})
        self.bpy.ops.wm.open_mainfile.assert_not_called()

    def test_initial_open_uses_fixed_safe_flags(self):
        target=self.root/"input.blend";target.write_bytes(b"BLENDER-v300"+b"fixture")
        digest=hashlib.sha256(target.read_bytes()).hexdigest()
        receipt=self.commands("blender.file.open_initial",{"path":"input.blend","expected_sha256":digest})
        self.bpy.ops.wm.open_mainfile.assert_called_once_with(filepath=str(target),load_ui=False,use_scripts=False)
        self.assertEqual(receipt["sha256"],digest)
        self.assertTrue(receipt["fresh_session"])

if __name__=="__main__":
    unittest.main()
