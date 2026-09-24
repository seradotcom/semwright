#!/usr/bin/env python3
"""Build a NEW unsigned app from explicit local release artifacts. No installation or signing."""
import argparse,json,pathlib,platform,shutil,subprocess
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--rust-dir',type=pathlib.Path,required=True)
p.add_argument('--native-library',type=pathlib.Path,required=True)
p.add_argument('--output',type=pathlib.Path,required=True)
a=p.parse_args()
if platform.system()!='Darwin':p.error('macOS with Xcode is required')
root=pathlib.Path(__file__).resolve().parents[1]
out=a.output.resolve()
if out.exists() or out.suffix!='.app':p.error('--output must be a NEW .app path')
arch=platform.machine()
if arch not in ('arm64','x86_64'):p.error('Unsupported architecture')
sources={n:(a.rust_dir/n).resolve(strict=True) for n in ('semwrightd','semwright','semwright-mcp','semwright-sandbox')}
sources['libSemwrightNative.dylib']=a.native_library.resolve(strict=True)
for n,s in sources.items():
 if not s.is_file():p.error('Expected a regular build artifact: '+n)
 got=subprocess.check_output(['xcrun','lipo','-archs',str(s)],text=True).strip().split()
 if arch not in got:p.error('Artifact architecture does not match build machine: '+n)
out.mkdir(parents=True)
mac=out/'Contents/MacOS';mac.mkdir(parents=True)
framework=out/'Contents/Frameworks';framework.mkdir()
resources=out/'Contents/Resources';resources.mkdir()
launch=out/'Contents/Library/LaunchAgents';launch.mkdir(parents=True)
shutil.copy2(root/'Info.plist',out/'Contents/Info.plist')
shutil.copy2(root/'org.semwright.agent.plist',launch/'org.semwright.agent.plist')
for name,s in sources.items():shutil.copy2(s,(framework if name.endswith('.dylib') else mac)/name)
subprocess.run(['xcrun','swiftc','-swift-version','5','-parse-as-library','-target',arch+'-apple-macosx14.0','-framework','AppKit','-framework','ServiceManagement',str(root/'Control.swift'),'-o',str(mac/'Semwright')],check=True)
for binary in mac.iterdir():
 linked=subprocess.check_output(['xcrun','otool','-L',str(binary)],text=True)
 if '@rpath/libSemwrightNative.dylib' in linked:
  commands=subprocess.check_output(['xcrun','otool','-l',str(binary)],text=True)
  if 'path @executable_path/../Frameworks ' not in commands:
   subprocess.run(['xcrun','install_name_tool','-add_rpath','@executable_path/../Frameworks',str(binary)],check=True)
(resources/'BUILD.json').write_text(json.dumps({'baseline':'963f0ceecb22ccfadf66b0937524fb30a6269030','architecture':arch,'signed':False,'notarized':False,'live_verified':False},indent=2)+'\n')
for s in mac.iterdir():s.chmod(0o755)
print('UNSIGNED, UNNOTARIZED app created: '+str(out))
