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

class FakeID(N):
    __hash__ = object.__hash__
    def as_pointer(self):
        return id(self)

def ident(**kw):
    values = dict(name="Fixture",users=1, use_fake_user=False, is_embedded_data=False,
                  library=None, override_library=None, animation_data=None)
    values.update(kw)
    return FakeID(**values)

def fixture():
    scene = ident(sequence_editor=None, use_nodes=False, rigidbody_world=None)
    obj = ident(type="MESH", modifiers=[], constraints=[], particle_systems=[],
                rigid_body=None, rigid_body_constraint=None, is_instancer=False)
    groups = dict(scenes=[scene], objects=[obj], meshes=[ident()], materials=[],
                  worlds=[], lights=[], images=[])
    data = N(**groups, user_map=Mock(return_value={}), filepath="original.blend",
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

    def add_all_ids(self):
        self.bpy.data.all_ids = [item for prop in self.bpy.data.bl_rna.properties
            if prop.type == "COLLECTION" for item in getattr(self.bpy.data,prop.identifier)]
        prop=N(type="COLLECTION",identifier="all_ids",is_readonly=True,fixed_type=N(identifier="ID"))
        self.bpy.data.bl_rna.properties.append(prop)
        return prop

    def test_all_ids_is_cross_checked_without_duplicate_orphan_retention(self):
        self.bpy.data.meshes[0].users=0
        self.add_all_ids()
        self.bpy.data.all_ids.reverse() # Aggregate ordering is not contractual.
        receipt=self.commands("blender.file.checkpoint",{"path":"recovery.blend"})
        self.assertEqual(receipt["retained_orphans"],[{"root":"meshes","name":"Fixture"}])
        self.assertFalse(self.bpy.data.meshes[0].use_fake_user)

    def test_all_ids_missing_extra_or_duplicate_data_rejected(self):
        self.add_all_ids()
        exact=list(self.bpy.data.all_ids)
        for altered in [exact[:-1],exact+[ident()],exact+[exact[0]]]:
            with self.subTest(size=len(altered)):
                self.bpy.data.all_ids=altered
                self.reject_before_save("Unsupported")

    def test_all_ids_invalid_rna_metadata_rejected(self):
        prop=self.add_all_ids()
        prop.is_readonly=False
        self.reject_before_save("Unsupported")
        prop.is_readonly=True
        prop.fixed_type.identifier="FutureID"
        self.reject_before_save("Unsupported")

    def test_mode_rejected_before_save(self):
        self.bpy.context.mode="EDIT_MESH"
        self.reject_before_save("Conflict")

    def test_dirty_image_and_unknown_category_rejected_before_save(self):
        self.bpy.data.images.append(ident(source="FILE",type="IMAGE",has_data=True,is_dirty=True))
        self.reject_before_save("Unsupported")
        self.bpy.data.images.clear()
        self.bpy.data.new_future_cache=[ident()]
        self.bpy.data.bl_rna.properties.append(N(type="COLLECTION",identifier="new_future_cache"))
        self.reject_before_save("Unsupported")

    def test_modifier_cache_rejected_before_save(self):
        self.bpy.data.objects[0].modifiers.append(N(type="CLOTH"))
        self.reject_before_save("Unsupported")

    def test_orphan_mesh_retained_in_copy_and_original_flag_restored(self):
        mesh=self.bpy.data.meshes[0];mesh.users=0
        original_save=self.bpy.ops.wm.save_as_mainfile.side_effect
        def observe(**kwargs):
            self.assertTrue(mesh.use_fake_user)
            return original_save(**kwargs)
        self.bpy.ops.wm.save_as_mainfile.side_effect=observe
        receipt=self.commands("blender.file.checkpoint",{"path":"recovery.blend"})
        self.assertFalse(mesh.use_fake_user)
        self.assertEqual(receipt["retained_orphans"],[{"root":"meshes","name":"Fixture"}])

    def test_empty_runtime_images_allowed_but_pixels_or_persistent_users_rejected(self):
        image=ident(source="VIEWER",type="RENDER_RESULT",has_data=False,is_dirty=False)
        self.bpy.data.images=[image]
        self.assertEqual(checkpoint_preflight(self.bpy)["empty_runtime_images"],1)
        image.has_data=True
        self.reject_before_save("Unsupported")
        image.has_data=False
        self.bpy.data.user_map.return_value={image:{ident(bl_rna=N(identifier="Material"))}}
        self.reject_before_save("Unsupported")

    def test_inspection_is_advisory_and_never_saves(self):
        self.bpy.context.mode="EDIT_MESH"
        result=self.commands("blender.file.checkpoint_inspect",{})
        self.assertFalse(result["supported"])
        self.assertEqual(result["code"],"Conflict")
        self.bpy.ops.wm.save_as_mainfile.assert_not_called()
        self.assertFalse(list(self.root.iterdir()))

    def test_palettes_require_finite_bounded_colours(self):
        palette=ident(colors=[N(color=[0.2,0.3,0.4])])
        self.bpy.data.palettes=[palette]
        self.bpy.data.bl_rna.properties.append(N(type="COLLECTION",identifier="palettes"))
        self.assertEqual(checkpoint_preflight(self.bpy)["palettes"],1)
        palette.colors[0].color[0]=float("nan")
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
        self.bpy.data.meshes[0].users=0
        self.bpy.ops.wm.save_as_mainfile.side_effect=OSError("bounded injected write failure")
        with self.assertRaises(CommandError) as error:
            self.commands("blender.file.checkpoint",{"path":"recovery.blend"})
        self.assertEqual(error.exception.code,"BackendFailed")
        self.assertEqual(self.bpy.context.preferences.filepaths.save_version,2)
        self.assertEqual(self.bpy.data.filepath,"original.blend")
        self.assertFalse(list(self.root.iterdir()))
        self.assertFalse(self.bpy.data.meshes[0].use_fake_user)

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
