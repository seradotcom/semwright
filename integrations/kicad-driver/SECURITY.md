# Seguridad, permisos y límites

## Frontera de confianza

El propietario controla manifest, digest ELF, configuración y named mounts. El contenido de KiCad y todos sus strings son datos no confiables. Un socket que responde como KiCad no constituye atestación de su binario. Un proceso malicioso del **mismo UID** puede simular un servidor; peer credentials + directorio privado + token de instancia no resuelven ese modelo de amenaza por completo.

No se ejecutan instrucciones contenidas en nombres de nets, footprints, zonas o proyectos. Los objetos proyectados incluyen `untrusted_content=true`; los strings permanecen en campos JSON. No aparecen en descripciones de capabilities ni se convierten en permisos, comandos o plantillas ejecutables. Los textos de error upstream no se propagan. Los errores son constantes acotadas.

## Controles ejecutados

| Riesgo | Control y prueba |
|---|---|
| Socket viejo/reemplazado | lstat, inode/device antes/después y antes de cada dispatch; invalidación de epoch; test de sustitución |
| Socket falso/otro usuario | tipo Unix socket, mismo UID, ancestros controlados, padre privado, SO_PEERCRED; no atestación de mismo UID |
| PID equivocado | expected_pid opcional, probado con mismatch; PID es relativo al namespace y no es identidad durable |
| Symlink/config público | O_NOFOLLOW, regular file, owner-only, fstat+SameFile, chequeo de ancestros; pruebas negativas |
| Token reflejado/logueado | String/GoString redactados, ningún log del request, filtro de egress si reaparecen bytes privados; pruebas |
| Protobuf malformado/grande | límites antes de asignación del cuerpo, parser bounded, tipo/ID/status; fuzz y tests |
| Peer colgado | deadline de socket; cierre del canal; sin retry |
| Resultado de mutación perdido | outcome_known=false, cuarentena, ref invalidada, no resend; fake aplica y pierde respuesta |
| Cross-project | filename/path exactos del DocumentSpecifier comparados con allowlist del propietario antes de BeginCommit |
| Lock o ref antigua | rechazo previo; fingerprint, epoch, documento, tipo, TTL, UUID y contenedor |
| Concurrencia | una operación por Engine; rechazo bounded probado por threads C ABI y race detector Go |
| Escape arbitrario | sin raw Any, raw NNG, shell, Python, delete, export, save ni arbitrary file capability |

## Archivo de configuración

Solamente la configuración fija recibe acceso directo de lectura. Debe ser un archivo regular privado (0600), del UID efectivo del driver, sin symlink, de hasta 64 KiB. El socket debe estar en un directorio privado (0700) de un propietario permitido. Se admite un ancestro sticky `/tmp` propiedad de root. No se cambian permisos automáticamente.

La configuración enumera como máximo 16 instancias y 32 documentos autorizados para mutación. Los paths son strings canónicos absolutos, acotados; los filenames de documento no pueden incluir directorios. Las rutas de proyecto vienen del servidor: **no se abren ni canonicalizan recorriendo proyectos personales**. La comparación por path no es una atestación de que un servidor malicioso diga la verdad.

Los controles lstat/fstat reducen carreras pero no equivalen a `openat2(RESOLVE_BENEATH)` en toda la cadena. Un adversario con el mismo UID y control de un ancestro permitido aún puede causar TOCTOU. La sandbox y la elección de raíces privadas por el propietario siguen siendo necesarias.

## Manifest y sandbox

El manifest pide `network=false`, un montaje readonly `kicad-config`, otro readonly `kicad-ipc`, y ningún proyecto ni path de salida. El socket se referencia dentro de `/workspace/kicad-ipc/`. La configuración no hereda `KICAD_API_TOKEN` ni `KICAD_API_SOCKET`, porque el host inspeccionado limpia el entorno.

Un montaje readonly de un Unix socket **no hace readonly la API del servidor**: la capacidad de enviar mensajes puede mutar la aplicación. Por eso importan tanto los permisos de capacidades como la allowlist de documentos y el driver fijado. La política del host debe permitir `driver:kicad` y, sólo para mutación, `kicad.modify`, con aprobación explícita.

No se ejecutó bubblewrap/Landlock aquí: no estaba instalado el helper/host ni bwrap. La compatibilidad cross-namespace del socket, traducción de UID/PID, bind readonly y reglas Landlock V3 tiene gate pendiente. No habilitar `network=true`, `--share-net`, montajes de `/tmp` completo, HOME o proyectos para hacer pasar el test.

## Incertidumbre y riesgos residuales importantes

No reintentar movimientos después de Timeout/BackendFailed incierto. Comprobar el documento disposable y el commit/undo en KiCad, reconciliar y sólo entonces reiniciar conscientemente el driver. La cuarentena está en memoria; un restart puede borrarla. El protocolo v1 no proporciona almacenamiento duradero de reconciliación.

Mover una pista/vía puede desconectar un circuito, violar separaciones o alterar el diseño. No hay DRC, autorización eléctrica ni garantía de manufacturabilidad. KiCad puede escribir autosaves aunque el driver no invoque guardado. Estas pruebas no abrieron un proyecto real.

La identidad de proyecto/objeto no permite detectar todas las recreaciones idénticas no observadas. El último read antes del write no es CAS. No se mueve ni rota un footprint por editar sólo su posición: sus hijos necesitan transformación coherente.

`go test -race` cubrió los tests Go, no certificó todas las llamadas concurrentes de un host real. La C ABI tuvo tests black-box independientes. No se ejecutaron cargo-audit, cargo-deny ni un escáner de vulnerabilidades del runtime Go. Sin real KiCad/NNG y sin sandbox real, **no hay declaración de seguridad de producción**.
