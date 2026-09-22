# Compatibilidad y exclusiones

| Dimensión | Estado |
|---|---|
| Linux amd64 | Referencia Go, biblioteca C ABI y ELF C+Go compilados y ejecutados |
| Rust SDK client Linux | Código presente; compilación pendiente |
| ARM64 Linux | No ejecutado; build.rs permite build nativo Linux, no certifica ARM |
| macOS / Windows | No implementados en este pack; ipc_linux.go usa SO_PEERCRED/syscall Linux |
| KiCad 9 / 10 | Proyección de comandos comunes implementada; validación fake, no live |
| Versiones anotadas, 9.99/10.99, 11 o desconocidas | Diagnóstico únicamente; no schematic/export implícito |
| NNG/libnng | SP/REQ-REP subset implementado directamente; interoperabilidad con libnng pendiente |
| Driver SDK snapshot v1 | Cliente fuente real + referencia wire; host real pendiente |
| Driver interfaces | health=true; dynamic/cancellation/events=false |
| Proyectos grandes | Se rechaza >1 MiB de respuesta o >4,096 objetos; no se promete streaming/inventario completo |

La paginación es de la proyección local: GetItems puede producir toda la respuesta del tipo solicitado. No se afirma paginación server-side. Las capacidades por versión constituyen una selección conservadora del baseline; un build concreto puede devolver Unsupported/Unavailable a una operación y se conserva ese rechazo. No existe una negociación completa de todos los comandos en este baseline.

La versión se lee numéricamente y se exige que `full_version` sea exactamente estable `major.minor.patch`, con major 9/10 y minor <90. Los builds de distribución anotados se dejan en diagnóstico en lugar de adivinar compatibilidad. Esto puede rechazar builds compatibles: es una limitación deliberada, visible y testeada.

El fingerprint utiliza bytes del objeto y el documento. No equivale a revision de KiCad. La selección múltiple de documentos devuelve AmbiguousTarget; no se elige por orden. El campo `editor` de status declara el alcance PCB del driver, no prueba que haya un documento abierto; esa observación se obtiene con document.current/list.
