# Licencias y procedencia

El paquete externo se distribuye conservadoramente como **GPL-3.0-or-later**. Véase GPL-3.0-or-later.txt. Esta elección evita presentar código que deriva/proyecta definiciones KiCad GPL como si estuviera inequívocamente libre de obligaciones para el core MIT/Apache de Semwright.

Se consultaron definiciones oficiales `.proto` con encabezados GPL-3.0-or-later. La implementación escribe un subconjunto wire y proyecciones propias; **no se incluyen archivos .proto oficiales completos ni bindings generados de ellos**. La fixture dinámica de Google Protobuf es autocontenida y autoredactada. La atribución y revisiones están en SOURCES.md.

| Material | Procedencia / tratamiento |
|---|---|
| KiCad envelope.proto | Revisión oficial 286b0611feca00727bf70bfa184ec2c28a745dc3; encabezado GPL3+; consultado, no vendorizado |
| Base commands | Revisión e9e4e7a3ff6c2413a1731b0cb61cfdc2b864a917; consultada |
| Board types | Revisión 5a3b2cc8fd4323d46bc3cea86530ec4b78e154f1 y source mirror 10.0.6; consultados |
| Editor/board commands | Source KiCad 10.0.6 reproducido por Fossies, identificado como mirror, sin checkout autenticado local |
| Driver SDK | Dependencia Git de Semwright, snapshot declarado MIT OR Apache-2.0; no se copió su implementación |
| Protocolo de compatibilidad v1 | Adaptador mínimo propio a partir de contratos observados; no fork del SDK |
| Go runtime/stdlib en binarios | BSD-style Go license incluida para distribuciones binarias; el ZIP no incorpora su código fuente ni toolchain |
| Designs y fake fixtures | Autoredactados para esta entrega, GPL3+ salvo licencias de herramientas externas |
| Tests Python | Herramientas jsonschema/protobuf no vendorizadas; deben conservar sus propias licencias al redistribuirlas |

Los bindings generados de material GPL pueden requerir análisis de obra derivada; no se afirma certeza jurídica ni que el hecho de generar código elimine obligaciones. Tampoco se infiere que un paquete tercero que etiqueta sus bindings como MIT haya resuelto esa cuestión. Mantener el driver separado es la recomendación de empaquetado, no un dictamen legal.

Antes de incluir código o distribuir el ELF Rust enlazado, resolver dependencias, ejecutar `cargo deny`/inventario de licencias y revisar la distribución completa, incluidos runtime/linkage y source obligations. No se ejecutaron esas auditorías aquí.
