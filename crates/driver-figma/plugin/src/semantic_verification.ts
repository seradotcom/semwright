type SemanticVisionMode = "protanopia" | "deuteranopia" | "tritanopia";

const SEMWRIGHT_VISION_MATRICES: Record<SemanticVisionMode, readonly number[][]> = {
  protanopia: [
    [0.152286, 1.052583, -0.204868],
    [0.114503, 0.786281, 0.099216],
    [-0.003882, -0.048116, 1.051998],
  ],
  deuteranopia: [
    [0.367322, 0.860646, -0.227968],
    [0.280085, 0.672501, 0.047413],
    [-0.011820, 0.042940, 0.968881],
  ],
  tritanopia: [
    [1.255528, -0.076749, -0.178779],
    [-0.078411, 0.930809, 0.147602],
    [0.004733, 0.691367, 0.303900],
  ],
};

function semanticVisionClamp(value: number): number {
  return Math.max(0, Math.min(1, value));
}

function semanticVisionLinear(value: number): number {
  return value <= 0.04045 ? value / 12.92 : Math.pow((value + 0.055) / 1.055, 2.4);
}

function semanticVisionSrgb(value: number): number {
  const bounded = semanticVisionClamp(value);
  return bounded <= 0.0031308
    ? bounded * 12.92
    : 1.055 * Math.pow(bounded, 1 / 2.4) - 0.055;
}

function semanticVisionColor(color: RGB, mode: SemanticVisionMode): RGB {
  const linear = [
    semanticVisionLinear(color.r),
    semanticVisionLinear(color.g),
    semanticVisionLinear(color.b),
  ];
  const matrix = SEMWRIGHT_VISION_MATRICES[mode];
  const mapped = matrix.map(row =>
    row[0] * linear[0] + row[1] * linear[1] + row[2] * linear[2]
  );
  return {
    r: semanticVisionSrgb(mapped[0]),
    g: semanticVisionSrgb(mapped[1]),
    b: semanticVisionSrgb(mapped[2]),
  };
}

function semanticVisionDistance(a: RGB, b: RGB): number {
  return Math.hypot(a.r - b.r, a.g - b.g, a.b - b.b);
}

function semanticVisionModes(value: unknown): SemanticVisionMode[] {
  const raw = value === undefined
    ? ["protanopia", "deuteranopia", "tritanopia"]
    : extraBoundedArray(value, 3, "vision_mode_limit").map(String);
  const seen = new Set<SemanticVisionMode>();
  for (const item of raw) {
    if (!["protanopia", "deuteranopia", "tritanopia"].includes(item)) {
      throw new Error("invalid_vision_mode");
    }
    seen.add(item as SemanticVisionMode);
  }
  if (!seen.size) throw new Error("empty_vision_modes");
  return [...seen];
}

function semanticVisionPaints(
  value: readonly Paint[] | PluginAPI["mixed"],
  mode: SemanticVisionMode,
): {paints: readonly Paint[] | PluginAPI["mixed"]; changed: number} {
  if (!Array.isArray(value)) return {paints: value, changed: 0};
  let changed = 0;
  const paints = value.map(paint => {
    if (paint.type !== "SOLID") return paint;
    changed++;
    return {...paint, color: semanticVisionColor(paint.color, mode)} as SolidPaint;
  });
  return {paints, changed};
}

function semanticVisionTransformSubtree(root: SceneNode, mode: SemanticVisionMode): number {
  let changed = 0;
  for (const base of extraWalk(root)) {
    if (!("visible" in base)) continue;
    const node = base as any;
    for (const field of ["fills", "strokes"]) {
      if (!(field in node)) continue;
      const transformed = semanticVisionPaints(node[field], mode);
      if (transformed.changed) {
        node[field] = transformed.paints;
        changed += transformed.changed;
      }
    }
    if (Array.isArray(node.effects)) {
      node.effects = node.effects.map((effect: any) => {
        if (!effect?.color || typeof effect.color.r !== "number") return effect;
        changed++;
        return {...effect, color: semanticVisionColor(effect.color, mode)};
      });
    }
  }
  return changed;
}

function semanticVisionColorJson(value: any) {
  return {
    r: Number(value.r),
    g: Number(value.g),
    b: Number(value.b),
    opacity: Number(value.opacity ?? 1),
  };
}
async function semanticVisionAnalyze(a: any) {
  const root = a.rootNodeId ? await nodeById(String(a.rootNodeId)) : figma.currentPage;
  const colors = extraColorUsage(root);
  const modes = semanticVisionModes(a.modes);
  const threshold = Math.max(0, Math.min(Math.sqrt(3), Number(a.threshold ?? 0.10)));
  const minOriginalDistance = Math.max(
    0,
    Math.min(Math.sqrt(3), Number(a.minOriginalDistance ?? 0.15)),
  );
  const maxPairs = Math.max(1, Math.min(500, Number(a.maxPairs ?? 200)));
  const results = modes.map(mode => {
    const simulated = colors.map(entry =>
      semanticVisionColor(entry.color as RGB, mode)
    );
    const pairs: any[] = [];
    let truncated = false;
    outer: for (let i = 0; i < colors.length; i++) {
      for (let j = i + 1; j < colors.length; j++) {
        const originalDistance = semanticVisionDistance(
          colors[i].color as RGB,
          colors[j].color as RGB,
        );
        const simulatedDistance = semanticVisionDistance(simulated[i], simulated[j]);
        if (originalDistance >= minOriginalDistance && simulatedDistance <= threshold) {
          pairs.push({
            colorA: semanticVisionColorJson(colors[i].color),
            colorB: semanticVisionColorJson(colors[j].color),
            simulatedA: simulated[i],
            simulatedB: simulated[j],
            originalDistance,
            simulatedDistance,
            nodeIdsA: colors[i].nodeIds,
            nodeIdsB: colors[j].nodeIds,
          });
          if (pairs.length >= maxPairs) {
            truncated = true;
            break outer;
          }
        }
      }
    }
    return {mode, pairs, truncated};
  });
  return {
    model: "machado-2009-full-severity",
    metric: "euclidean-srgb",
    rootNodeId: root.id,
    colorCount: colors.length,
    threshold,
    minOriginalDistance,
    results,
  };
}

async function semanticVisionPreview(a: any) {
  const source = asScene(await nodeById(String(a.nodeId)));
  const modes = semanticVisionModes(a.modes);
  const gap = Math.max(0, Math.min(100_000, Number(a.gap ?? 80)));
  const prefix = String(a.namePrefix ?? "Semwright vision").slice(0, 64);
  const sourceAny = source as any;
  if (typeof sourceAny.clone !== "function") throw new Error("node_not_cloneable");
  const previews: any[] = [];
  const width = typeof sourceAny.width === "number" ? sourceAny.width : 0;
  const x = typeof sourceAny.x === "number" ? sourceAny.x : 0;
  const y = typeof sourceAny.y === "number" ? sourceAny.y : 0;
  for (let index = 0; index < modes.length; index++) {
    const mode = modes[index];
    const clone = sourceAny.clone() as SceneNode;
    clone.name = `[${prefix}:${mode}] ${source.name}`.slice(0, 256);
    const cloneAny = clone as any;
    if (typeof cloneAny.x === "number") cloneAny.x = x + (width + gap) * (index + 1);
    if (typeof cloneAny.y === "number") cloneAny.y = y;
    const transformedPaints = semanticVisionTransformSubtree(clone, mode);
    previews.push({
      mode,
      node: summarize(clone),
      transformedPaints,
    });
  }
  return {sourceNodeId: source.id, previews};
}

async function semanticVerifyNode(a: any) {
  const node = asScene(await nodeById(String(a.nodeId)));
  const scale = Math.max(0.1, Math.min(4, Number(a.scale ?? 1)));
  const bytes = await (node as any).exportAsync({
    format: "PNG",
    constraint: {type: "SCALE", value: scale},
  } as ExportSettingsImage);
  const name = String(
    a.name ?? `figma-verify-${node.id.replace(/[^A-Za-z0-9._-]/g, "-")}.png`,
  ).slice(0, 256);
  const artifact = extraStoreArtifact(bytes, "image/png", name);
  const structure = await tree(node);
  const nodeCount = extraWalk(node).length;
  return {
    ...artifact,
    nodeId: node.id,
    scale,
    nodeCount,
    structure,
  };
}

async function handleSemanticVerification(
  request: BridgeRequest,
  a: any,
): Promise<BridgeResponse | null> {
  switch (request.operation) {
    case "a11y.vision.analyze":
      return ok(request.id, await semanticVisionAnalyze(a));
    case "a11y.vision.preview":
      return ok(request.id, await semanticVisionPreview(a), true);
    case "verify.node":
      return ok(request.id, await semanticVerifyNode(a));
    default:
      return null;
  }
}
