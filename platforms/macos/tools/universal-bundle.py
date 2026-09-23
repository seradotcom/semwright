#!/usr/bin/env python3
"""Merge matching UNSIGNED ARM64 and Intel bundles. Never reuse a signature after lipo."""
import argparse,pathlib,platform,plistlib,shutil,subprocess,json
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--arm64',type=pathlib.Path,required=True);p.add_argument('--intel',type=pathlib.Path,required=True);p.add_argument('--output',type=pathlib.Path,required=True)
a=p.parse_args()
if platform.system()!='Darwin':p.error('lipo on macOS is required')
a.arm64=a.arm64.resolve(strict=True);a.intel=a.intel.resolve(strict=True);a.output=a.output.resolve()
if a.output.exists() or a.output.suffix!='.app':p.error('Output must be a NEW .app')
if any((s/'Contents/_CodeSignature').exists() for s in (a.arm64,a.intel)):p.error('Use unsigned input bundles')
if (a.arm64/'Contents/Info.plist').read_bytes()!=(a.intel/'Contents/Info.plist').read_bytes():p.error('Bundle identities differ')
files=['Contents/MacOS/'+n for n in ('Semwright','semwrightd','semwright','semwright-mcp','semwright-sandbox')]+['Contents/Frameworks/libSemwrightNative.dylib']
for path in files:
 for base,arch in ((a.arm64,'arm64'),(a.intel,'x86_64')):
  got=subprocess.check_output(['xcrun','lipo','-archs',str(base/path)],text=True).split()
  if got!=[arch]:p.error('Expected a single-architecture unsigned slice: '+path)
shutil.copytree(a.arm64,a.output,symlinks=False)
for path in files:
 subprocess.run(['xcrun','lipo','-create',str(a.arm64/path),str(a.intel/path),'-output',str(a.output/path)],check=True)
 subprocess.run(['xcrun','lipo','-verify_arch','arm64','x86_64',str(a.output/path)],check=True)
meta=a.output/'Contents/Resources/BUILD.json';data=json.loads(meta.read_text());data['architecture']='universal2';meta.write_text(json.dumps(data,indent=2)+'\n')
print('UNSIGNED universal2 bundle created; re-sign every nested executable before distribution')
