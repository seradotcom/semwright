# Semwright — KiCad driver reference pack

**Estado: el cliente Rust, el núcleo Go/C ABI y sus pruebas forman parte del workspace y
compilan con el SDK actual. La interoperabilidad con KiCad real continúa pendiente.**

Esta integración conserva su frontera y licencia GPL-3.0-or-later dentro de
`integrations/kicad-driver`; no forma parte del código core MIT/Apache.

## Qué contiene realmente

`driver/` implementa el trait y `serve` del `semwright-driver-sdk` del workspace.
`kicad-ipc/` enlaza en build time el núcleo Go mediante C ABI. `native/internal/core`
implementa operaciones semánticas, catálogos, seguridad, referencias y commits;
`native/internal/wire` implementa el subconjunto acotado de protobuf y SP IPC/REQ-REP.
El driver de producción no ejecuta Python, shell ni helpers en tiempo de ejecución.

El fake Python sólo pertenece a las pruebas de interoperabilidad y no se distribuye como parte
del ejecutable. Pasar ese fake no equivale a demostrar compatibilidad con KiCad o libnng reales.

## Resultado verificable

| Componente | Resultado de esta entrega |
|---|---|
| Núcleo nativo y referencia ELF | Compilados y ejecutados |
| Unit tests Go | 35 aprobados; además 6 funciones fuzz con sus seeds |
| Python | 45 aprobados: 33 integración/fake/C ABI y 12 contratos/goldens/negativos |
| Race detector Go / go vet / gofmt | Aprobados en el paquete de procedencia; CI vuelve a ejecutar los tests nativos |
| Fuzzing Go | 6 targets, 209,967 ejecuciones observadas, sin crashes encontrados |
| Cobertura de caja negra | 79.5% de sentencias Go instrumentadas; no incluye Rust ni el entrypoint C ABI |
| Cliente Rust / Cargo / Clippy | Integrado y ejecutado con el lock del workspace |
| Semwright Host real / sandbox real | Cubierto en CI contra el fake IPC independiente |
| KiCad real / libnng real | PENDIENTES; no estaban instalados |

La capa funcional ofrece **23 capabilities** cuando conecta a una versión estable admitida y el propietario habilita mutaciones; **19** en modo sólo lectura. Una versión desconocida conectada recibe 5 operaciones diagnósticas; sin conexión, 3. El catálogo permanece congelado durante la vida del proceso.

## Reproducir sin tocar Semwright

Requisitos de las pruebas nativas: Linux, Go 1.23+, C compiler, Python con `jsonschema` y `protobuf`. Las versiones ejecutadas fueron Go 1.23.2, GCC 14.2.0 y Python 3.13.5. No hay dependencias externas de módulos Go.

```bash
python3 -m pip install -r tests/requirements.txt
LOG_DIR=/tmp/kicad-native-logs scripts/verify-native.sh
SECONDS_PER_TARGET=3 scripts/fuzz-smoke.sh
```

Todas las conexiones de esas pruebas son a fixtures creados dentro de directorios temporales privados. No buscan proyectos KiCad existentes. `scripts/verify-native.sh` elimina sus ejecutables temporales.

Para compilar y probar la integración:

```bash
cargo test --locked -p kicad-ipc -p semwright-kicad-driver
cargo clippy --locked -p kicad-ipc -p semwright-kicad-driver --all-targets -- -D warnings
cargo build --locked --release -p semwright-kicad-driver
```

La integración utiliza el `Cargo.lock` resuelto del workspace. No conserva ni acepta la semilla
de lock provisional del paquete original.

## Lecturas esenciales

[SECURITY.md](SECURITY.md) enumera límites y riesgos residuales;
[COMPATIBILITY.md](COMPATIBILITY.md) documenta las versiones estudiadas y
[CAPABILITIES.md](CAPABILITIES.md) describe la superficie curada. La prueba con KiCad real
continúa siendo un gate pendiente y no se sustituye con el servidor fake.

No se exponen movimiento/rotación de footprints, eliminación, ejecución arbitraria, guardado, exportación ni schematic IPC. Sí existen movimiento acotado de track/via y selección. Mover pistas o vías puede romper conectividad eléctrica: no se ejecuta DRC ni se garantiza un diseño eléctricamente válido.

## Distribución y licencia

Este subárbol permanece **GPL-3.0-or-later** y no se relicencia bajo MIT/Apache. Véase
[LICENSES/NOTICE.md](LICENSES/NOTICE.md). No se incluyen binarios, sockets, cachés ni
credenciales; el driver se compila desde fuentes y se fija por SHA-256 en el manifiesto owner.
