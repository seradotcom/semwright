# START HERE — Linux Agentic Computer Blueprint

Fecha de referencia: 2026-09-21.

Este paquete define el trabajo para un chat nuevo: construir un proyecto open source de calidad de producción para controlar un escritorio Linux de forma **semántica, determinista, auditable y extensible**, priorizando APIs estructuradas sobre visión.

## Qué debes hacer en el chat nuevo

1. Sube este ZIP completo al chat nuevo.
2. Dile al agente que lea **todos** los documentos antes de escribir código.
3. Pega el contenido de `MASTER_PROMPT.md` como instrucción principal.
4. No aceptes una entrega que sea sólo arquitectura, scaffolding o una demo.
5. La entrega pedida al nuevo chat es un **repositorio completo y comprimido en ZIP**, listo para probar en una máquina Linux real.
6. Si el entorno del chat no dispone de GNOME/KDE/Wayland real, el agente debe:
   - compilar y ejecutar todos los tests que sí puedan ejecutarse;
   - construir backends falsos y harnesses headless;
   - no afirmar que probó lo que no probó;
   - documentar claramente los pasos de validación manual restantes.

## Idea central

El sistema NO será otro agente que haga:

`screenshot -> LLM -> coordenadas -> click -> screenshot`.

La jerarquía obligatoria es:

1. API específica de la aplicación.
2. D-Bus / system service / compositor API.
3. AT-SPI accessibility tree.
4. XDG Desktop Portal / libei / input backend.
5. Input sintético de bajo nivel.
6. Screenshot + visión, sólo como último fallback opcional.

El proyecto debe permitir que un agente piense en acciones como:

```text
window.focus(app="org.gimp.GIMP")
ui.find(role="button", name="Export")
ui.invoke(ref="ui:...")
blender.object.create(...)
recipe.run("export-mobile-assets")
```

y no en píxeles.

## Documentos del paquete

- `MASTER_PROMPT.md`: prompt autosuficiente para el chat nuevo.
- `PRODUCT_VISION.md`: qué producto estamos construyendo y qué lo diferencia.
- `ARCHITECTURE.md`: arquitectura propuesta de producción.
- `BACKENDS_AND_COMPATIBILITY.md`: estrategia Wayland/X11/DE/compositor.
- `COMMANDS_AND_PLUGIN_SDK.md`: command model, CLI, MCP, recipes y plugins.
- `SECURITY_THREAT_MODEL.md`: permisos, sandboxing, secretos, confirmaciones y auditoría.
- `REPO_BLUEPRINT.md`: estructura concreta del monorepo y responsabilidades.
- `TESTING_RELEASE_AND_QUALITY.md`: pruebas, CI, packaging, release y criterios de calidad.
- `ACCEPTANCE_CHECKLIST.md`: definición de “terminado”.
- `RESEARCH_BASELINE_2026-09-21.md`: baseline técnico y fuentes verificadas.

## Regla de producto

La meta no es “hacer algo que se vea impresionante en un video”. La meta es construir una herramienta que un desarrollador pueda instalar, entender, auditar, extender y confiar en ella.

“Puede llegar a 10k stars” se interpreta como estándar de producto, documentación y utilidad general; **no** como métrica garantizable.
