type BridgeRequest = {
  id: string;
  sessionId: string;
  generation: number;
  expectedRevision?: number;
  operation: string;
  args: Record<string, unknown>;
};

type BridgeResponse = {
  id: string;
  ok: boolean;
  revision: number;
  value?: unknown;
  error?: {code: string; message: string; outcomeKnown: boolean};
};

let revision = 0;
const MAX_RESULTS = 200;
const MAX_TREE = 2000;
const MAX_DEPTH = 32;
const MAX_KEYFRAMES = 1024;

function fail(id: string, code: string, message: string, outcomeKnown = true): BridgeResponse {
  return {id, ok: false, revision, error: {code, message, outcomeKnown}};
}

function ok(id: string, value: unknown, mutated = false): BridgeResponse {
  if (mutated) revision++;
  return {id, ok: true, revision, value};
}

async function nodeById(id: string): Promise<BaseNode> {
  const node = await figma.getNodeByIdAsync(id);
  if (!node) throw new Error("node_not_found");
  return node;
}

async function pageById(id: string): Promise<PageNode> {
  const node = await nodeById(id);
  if (node.type !== "PAGE") throw new Error("not_page");
  await node.loadAsync();
  return node;
}

function asScene(node: BaseNode): SceneNode {
  if (!("visible" in node)) throw new Error("not_scene_node");
  return node as SceneNode;
}

function summarize(node: BaseNode): Record<string, unknown> {
  const result: Record<string, unknown> = {id: node.id, type: node.type, name: node.name};
  if ("visible" in node) result.visible = node.visible;
  if ("locked" in node) result.locked = node.locked;
  if ("x" in node) {
    result.x = node.x;
    result.y = node.y;
    result.width = node.width;
    result.height = node.height;
  }
  if ("rotation" in node) result.rotation = node.rotation;
  if ("opacity" in node) result.opacity = node.opacity;
  if ("layoutMode" in node) result.layoutMode = node.layoutMode;
  return result;
}

async function tree(root: BaseNode, depth = 0, budget = {n: 0}): Promise<unknown> {
  if (depth > MAX_DEPTH || budget.n >= MAX_TREE) return {truncated: true};
  budget.n++;
  const result = summarize(root) as Record<string, unknown>;
  if ("children" in root) {
    const children: unknown[] = [];
    for (const child of root.children) {
      if (budget.n >= MAX_TREE) {
        children.push({truncated: true});
        break;
      }
      children.push(await tree(child, depth + 1, budget));
    }
    result.children = children;
  }
  return result;
}

function childrenOf(node: BaseNode): readonly SceneNode[] {
  if (!("children" in node)) return [];
  return node.children as readonly SceneNode[];
}

function ancestorsOf(node: BaseNode): Record<string, unknown>[] {
  const result: Record<string, unknown>[] = [];
  let current: BaseNode | null = node.parent;
  while (current && result.length < MAX_DEPTH) {
    result.push(summarize(current));
    current = current.parent;
  }
  return result;
}

function searchCurrentPage(args: any): Record<string, unknown>[] {
  const name = typeof args.name === "string" ? args.name.toLowerCase() : null;
  const type = typeof args.nodeType === "string" ? args.nodeType.toUpperCase() : null;
  const limit = Math.min(Number(args.limit ?? 50), MAX_RESULTS);
  const result: Record<string, unknown>[] = [];
  const stack: BaseNode[] = [...figma.currentPage.children].reverse();
  let visited = 0;
  while (stack.length && result.length < limit && visited < MAX_TREE) {
    const node = stack.pop()!;
    visited++;
    const nameMatch = !name || node.name.toLowerCase().includes(name);
    const typeMatch = !type || node.type === type;
    if (nameMatch && typeMatch) result.push(summarize(node));
    if ("children" in node) {
      for (let i = node.children.length - 1; i >= 0; i--) stack.push(node.children[i]);
    }
  }
  return result;
}

function applyBasicSceneArgs(node: SceneNode, args: any) {
  if (typeof args.name === "string") node.name = args.name.slice(0, 256);
  if ("x" in node && args.x !== undefined) {
    const x = Number(args.x);
    if (!Number.isFinite(x)) throw new Error("invalid_x");
    (node as any).x = x;
  }
  if ("y" in node && args.y !== undefined) {
    const y = Number(args.y);
    if (!Number.isFinite(y)) throw new Error("invalid_y");
    (node as any).y = y;
  }
  if ((args.width !== undefined || args.height !== undefined) && "resize" in node) {
    const width = args.width === undefined ? (node as any).width : Number(args.width);
    const height = args.height === undefined ? (node as any).height : Number(args.height);
    if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) {
      throw new Error("invalid_size");
    }
    (node as any).resize(width, height);
  }
}

function patchScene(node: SceneNode, args: any) {
  if (args.name !== undefined) node.name = String(args.name).slice(0, 256);
  if (args.visible !== undefined) node.visible = Boolean(args.visible);
  if (args.locked !== undefined) node.locked = Boolean(args.locked);
  if ("opacity" in node && args.opacity !== undefined) {
    const opacity = Number(args.opacity);
    if (!Number.isFinite(opacity) || opacity < 0 || opacity > 1) throw new Error("invalid_opacity");
    (node as any).opacity = opacity;
  }
  if ("x" in node && args.x !== undefined) (node as any).x = Number(args.x);
  if ("y" in node && args.y !== undefined) (node as any).y = Number(args.y);
}

async function ensureFonts(node: TextNode) {
  const fonts = node.getRangeAllFontNames(0, node.characters.length);
  const seen = new Set<string>();
  for (const font of fonts) {
    const key = font.family + "\0" + font.style;
    if (!seen.has(key)) {
      seen.add(key);
      await figma.loadFontAsync(font);
    }
  }
}

function rejectUnsafeSvg(svg: string) {
  if (svg.length > 1_048_576) throw new Error("svg_too_large");
  if (/<script\b|<foreignObject\b|\bon\w+\s*=|(?:href|src)\s*=\s*["']https?:/i.test(svg)) {
    throw new Error("unsafe_svg");
  }
}

function layoutSummary(node: SceneNode): Record<string, unknown> {
  const n = node as any;
  if (!("layoutMode" in n)) throw new Error("layout_unavailable");
  return {
    layoutMode: n.layoutMode,
    primaryAxisSizingMode: n.primaryAxisSizingMode,
    counterAxisSizingMode: n.counterAxisSizingMode,
    primaryAxisAlignItems: n.primaryAxisAlignItems,
    counterAxisAlignItems: n.counterAxisAlignItems,
    itemSpacing: n.itemSpacing,
    paddingTop: n.paddingTop,
    paddingRight: n.paddingRight,
    paddingBottom: n.paddingBottom,
    paddingLeft: n.paddingLeft,
    layoutWrap: n.layoutWrap,
  };
}

function patchLayout(node: SceneNode, args: any) {
  const n = node as any;
  if (!("layoutMode" in n)) throw new Error("layout_unavailable");
  for (const field of [
    "layoutMode",
    "primaryAxisSizingMode",
    "counterAxisSizingMode",
    "primaryAxisAlignItems",
    "counterAxisAlignItems",
    "layoutWrap",
  ]) {
    if (args[field] !== undefined) n[field] = args[field];
  }
  for (const field of ["itemSpacing", "paddingTop", "paddingRight", "paddingBottom", "paddingLeft"]) {
    if (args[field] !== undefined) {
      const value = Number(args[field]);
      if (!Number.isFinite(value)) throw new Error("invalid_layout_number");
      n[field] = value;
    }
  }
}

function solidPaint(args: any): SolidPaint {
  const r = Number(args.r), g = Number(args.g), b = Number(args.b);
  const opacity = args.opacity === undefined ? 1 : Number(args.opacity);
  if (![r, g, b, opacity].every(Number.isFinite) || r < 0 || r > 1 || g < 0 || g > 1 || b < 0 || b > 1 || opacity < 0 || opacity > 1) {
    throw new Error("invalid_paint");
  }
  return {type: "SOLID", color: {r, g, b}, opacity};
}

function textSummary(node: TextNode): Record<string, unknown> {
  return {
    ...summarize(node),
    characters: node.characters.slice(0, 65_536),
    fontName: node.fontName === figma.mixed ? "MIXED" : node.fontName,
    fontSize: node.fontSize === figma.mixed ? "MIXED" : node.fontSize,
    textAlignHorizontal: node.textAlignHorizontal,
    textAlignVertical: node.textAlignVertical,
    textAutoResize: node.textAutoResize,
  };
}

async function componentSummary(node: ComponentNode): Promise<Record<string, unknown>> {
  return {
    ...summarize(node),
    key: node.key,
    description: node.description,
    properties: node.componentPropertyDefinitions,
  };
}

async function instanceSummary(node: InstanceNode): Promise<Record<string, unknown>> {
  const main = await node.getMainComponentAsync();
  return {
    ...summarize(node),
    mainComponent: main ? {id: main.id, name: main.name, key: main.key} : null,
    componentProperties: node.componentProperties,
    scaleFactor: node.scaleFactor,
  };
}

async function extractDesignSystem(): Promise<Record<string, unknown>> {
  const [collections, variables, paintStyles, textStyles, effectStyles, gridStyles] = await Promise.all([
    figma.variables.getLocalVariableCollectionsAsync(),
    figma.variables.getLocalVariablesAsync(),
    figma.getLocalPaintStylesAsync(),
    figma.getLocalTextStylesAsync(),
    figma.getLocalEffectStylesAsync(),
    figma.getLocalGridStylesAsync(),
  ]);
  const components: Record<string, unknown>[] = [];
  const stack: BaseNode[] = [...figma.currentPage.children].reverse();
  let visited = 0;
  while (stack.length && visited < MAX_TREE && components.length < MAX_RESULTS) {
    const node = stack.pop()!;
    visited++;
    if (node.type === "COMPONENT") components.push(await componentSummary(node));
    if (node.type === "COMPONENT_SET") {
      components.push({
        ...summarize(node),
        variants: node.children.slice(0, MAX_RESULTS).map(summarize),
      });
    }
    if ("children" in node) {
      for (let i = node.children.length - 1; i >= 0; i--) stack.push(node.children[i]);
    }
  }
  return {
    collections: collections.slice(0, MAX_RESULTS).map(c => ({
      id: c.id, name: c.name, modes: c.modes, defaultModeId: c.defaultModeId, variableIds: c.variableIds,
    })),
    variables: variables.slice(0, MAX_RESULTS).map(v => ({
      id: v.id, name: v.name, resolvedType: v.resolvedType, valuesByMode: v.valuesByMode,
    })),
    components,
    styles: {
      paint: paintStyles.slice(0, MAX_RESULTS).map(s => ({id: s.id, name: s.name, key: s.key})),
      text: textStyles.slice(0, MAX_RESULTS).map(s => ({id: s.id, name: s.name, key: s.key})),
      effect: effectStyles.slice(0, MAX_RESULTS).map(s => ({id: s.id, name: s.name, key: s.key})),
      grid: gridStyles.slice(0, MAX_RESULTS).map(s => ({id: s.id, name: s.name, key: s.key})),
    },
  };
}

async function handle(request: BridgeRequest): Promise<BridgeResponse> {
  if (request.expectedRevision !== undefined && request.expectedRevision !== revision) {
    return fail(request.id, "conflict", "document revision changed");
  }
  const a = request.args as any;
  try {
    switch (request.operation) {
      case "document.status":
        return ok(request.id, {documentId: figma.root.id, editorType: figma.editorType, revision, currentPage: figma.currentPage.id});
      case "document.inspect":
        return ok(request.id, {documentId: figma.root.id, editorType: figma.editorType, revision, pageCount: figma.root.children.length, currentPage: summarize(figma.currentPage)});

      case "page.list":
        return ok(request.id, figma.root.children.slice(0, MAX_RESULTS).map(summarize));
      case "page.create": {
        const page = figma.createPage();
        if (a.name) page.name = String(a.name).slice(0, 256);
        return ok(request.id, summarize(page), true);
      }
      case "page.inspect":
        return ok(request.id, await tree(await pageById(String(a.pageId))));
      case "page.rename": {
        const page = await pageById(String(a.pageId));
        page.name = String(a.name).slice(0, 256);
        return ok(request.id, summarize(page), true);
      }
      case "page.remove": {
        const page = await pageById(String(a.pageId));
        if (page.id === figma.currentPage.id) throw new Error("cannot_remove_current_page");
        page.remove();
        return ok(request.id, {removed: true}, true);
      }
      case "page.current.get":
        return ok(request.id, summarize(figma.currentPage));
      case "page.current.set": {
        const page = await pageById(String(a.pageId));
        await figma.setCurrentPageAsync(page);
        return ok(request.id, summarize(page), true);
      }

      case "selection.get":
        return ok(request.id, figma.currentPage.selection.slice(0, MAX_RESULTS).map(summarize));
      case "selection.set": {
        const ids = a.nodeIds as string[];
        if (!Array.isArray(ids) || ids.length > MAX_RESULTS) throw new Error("selection_limit");
        const nodes: SceneNode[] = [];
        for (const id of ids) nodes.push(asScene(await nodeById(String(id))));
        figma.currentPage.selection = nodes;
        return ok(request.id, {count: nodes.length}, true);
      }
      case "selection.clear":
        figma.currentPage.selection = [];
        return ok(request.id, {count: 0}, true);

      case "node.get":
        return ok(request.id, summarize(await nodeById(String(a.nodeId))));
      case "node.children":
        return ok(request.id, childrenOf(await nodeById(String(a.nodeId))).slice(0, MAX_RESULTS).map(summarize));
      case "node.ancestors":
        return ok(request.id, ancestorsOf(await nodeById(String(a.nodeId))));
      case "node.tree":
        return ok(request.id, await tree(await nodeById(String(a.nodeId))));
      case "node.search":
        return ok(request.id, searchCurrentPage(a));
      case "node.patch": {
        const node = asScene(await nodeById(String(a.nodeId)));
        patchScene(node, a);
        return ok(request.id, summarize(node), true);
      }
      case "node.rename": {
        const node = await nodeById(String(a.nodeId));
        node.name = String(a.name).slice(0, 256);
        return ok(request.id, summarize(node), true);
      }
      case "node.move": {
        const node = asScene(await nodeById(String(a.nodeId)));
        (node as any).x = Number(a.x);
        (node as any).y = Number(a.y);
        return ok(request.id, summarize(node), true);
      }
      case "node.resize": {
        const node = asScene(await nodeById(String(a.nodeId)));
        if (!("resize" in node)) throw new Error("not_resizable");
        (node as any).resize(Number(a.width), Number(a.height));
        return ok(request.id, summarize(node), true);
      }
      case "node.rotate": {
        const node = asScene(await nodeById(String(a.nodeId)));
        if (!("rotation" in node)) throw new Error("not_rotatable");
        (node as any).rotation = Number(a.rotation);
        return ok(request.id, summarize(node), true);
      }
      case "node.remove": {
        const node = await nodeById(String(a.nodeId));
        node.remove();
        return ok(request.id, {removed: true}, true);
      }
      case "node.clone": {
        const node = asScene(await nodeById(String(a.nodeId)));
        if (!("clone" in node)) throw new Error("not_clonable");
        return ok(request.id, summarize((node as any).clone()), true);
      }
      case "node.reparent": {
        const node = asScene(await nodeById(String(a.nodeId)));
        const parent = await nodeById(String(a.parentId));
        if (!("appendChild" in parent)) throw new Error("parent_cannot_have_children");
        (parent as BaseNode & ChildrenMixin).appendChild(node);
        return ok(request.id, summarize(node), true);
      }
      case "node.reorder": {
        const node = asScene(await nodeById(String(a.nodeId)));
        const parent = node.parent;
        if (!parent || !("insertChild" in parent)) throw new Error("parent_cannot_reorder");
        (parent as BaseNode & ChildrenMixin).insertChild(Number(a.index), node);
        return ok(request.id, summarize(node), true);
      }

      case "frame.create": {
        const node = figma.createFrame(); applyBasicSceneArgs(node, a); return ok(request.id, summarize(node), true);
      }
      case "section.create": {
        const node = figma.createSection(); applyBasicSceneArgs(node, a); return ok(request.id, summarize(node), true);
      }
      case "rect.create": {
        const node = figma.createRectangle(); applyBasicSceneArgs(node, a); return ok(request.id, summarize(node), true);
      }
      case "ellipse.create": {
        const node = figma.createEllipse(); applyBasicSceneArgs(node, a); return ok(request.id, summarize(node), true);
      }
      case "line.create": {
        const node = figma.createLine(); applyBasicSceneArgs(node, a); return ok(request.id, summarize(node), true);
      }
      case "polygon.create": {
        const node = figma.createPolygon(); applyBasicSceneArgs(node, a); return ok(request.id, summarize(node), true);
      }
      case "star.create": {
        const node = figma.createStar(); applyBasicSceneArgs(node, a); return ok(request.id, summarize(node), true);
      }

      case "layout.inspect":
        return ok(request.id, layoutSummary(asScene(await nodeById(String(a.nodeId)))));
      case "layout.patch": {
        const node = asScene(await nodeById(String(a.nodeId)));
        patchLayout(node, a);
        return ok(request.id, layoutSummary(node), true);
      }
      case "paint.patch": {
        const node = asScene(await nodeById(String(a.nodeId))) as any;
        if (!("fills" in node)) throw new Error("fills_unavailable");
        node.fills = [solidPaint(a)];
        return ok(request.id, summarize(node), true);
      }
      case "stroke.patch": {
        const node = asScene(await nodeById(String(a.nodeId))) as any;
        if (!("strokes" in node)) throw new Error("strokes_unavailable");
        node.strokes = [solidPaint(a)];
        if (a.weight !== undefined && "strokeWeight" in node) node.strokeWeight = Number(a.weight);
        return ok(request.id, summarize(node), true);
      }
      case "effects.patch": {
        const node = asScene(await nodeById(String(a.nodeId))) as any;
        if (!("effects" in node)) throw new Error("effects_unavailable");
        if (!Array.isArray(a.effects) || a.effects.length > 32) throw new Error("effects_limit");
        node.effects = a.effects as Effect[];
        return ok(request.id, summarize(node), true);
      }

      case "text.create": {
        const node = figma.createText();
        await figma.loadFontAsync(node.fontName as FontName);
        node.characters = String(a.characters ?? "");
        if (typeof a.name === "string") node.name = a.name.slice(0, 256);
        return ok(request.id, textSummary(node), true);
      }
      case "text.inspect": {
        const node = await nodeById(String(a.nodeId));
        if (node.type !== "TEXT") throw new Error("not_text");
        return ok(request.id, textSummary(node));
      }
      case "text.patch": {
        const node = await nodeById(String(a.nodeId));
        if (node.type !== "TEXT") throw new Error("not_text");
        await ensureFonts(node);
        if (a.characters !== undefined) node.characters = String(a.characters);
        if (a.name !== undefined) node.name = String(a.name).slice(0, 256);
        return ok(request.id, textSummary(node), true);
      }
      case "svg.import": {
        const svg = String(a.svg); rejectUnsafeSvg(svg); return ok(request.id, summarize(figma.createNodeFromSvg(svg)), true);
      }

      case "component.create": {
        const node = figma.createComponent();
        if (typeof a.name === "string") node.name = a.name.slice(0, 256);
        return ok(request.id, await componentSummary(node), true);
      }
      case "component.from_node": {
        const node = asScene(await nodeById(String(a.nodeId)));
        return ok(request.id, await componentSummary(figma.createComponentFromNode(node)), true);
      }
      case "component.inspect": {
        const node = await nodeById(String(a.nodeId));
        if (node.type !== "COMPONENT") throw new Error("not_component");
        return ok(request.id, await componentSummary(node));
      }
      case "component_set.create": {
        const ids = a.componentIds as string[];
        if (!Array.isArray(ids) || ids.length < 2 || ids.length > 64) throw new Error("component_count");
        const components: ComponentNode[] = [];
        for (const id of ids) {
          const node = await nodeById(String(id));
          if (node.type !== "COMPONENT") throw new Error("not_component");
          components.push(node);
        }
        const set = figma.combineAsVariants(components, figma.currentPage);
        if (typeof a.name === "string") set.name = a.name.slice(0, 256);
        return ok(request.id, {...summarize(set), variants: set.children.map(summarize)}, true);
      }
      case "component_set.inspect": {
        const node = await nodeById(String(a.nodeId));
        if (node.type !== "COMPONENT_SET") throw new Error("not_component_set");
        return ok(request.id, {...summarize(node), variants: node.children.slice(0, MAX_RESULTS).map(summarize)});
      }
      case "variant.list": {
        const node = await nodeById(String(a.nodeId));
        if (node.type !== "COMPONENT_SET") throw new Error("not_component_set");
        return ok(request.id, node.children.slice(0, MAX_RESULTS).map(summarize));
      }
      case "instance.create": {
        const node = await nodeById(String(a.componentId));
        if (node.type !== "COMPONENT") throw new Error("not_component");
        return ok(request.id, await instanceSummary(node.createInstance()), true);
      }
      case "instance.inspect": {
        const node = await nodeById(String(a.nodeId));
        if (node.type !== "INSTANCE") throw new Error("not_instance");
        return ok(request.id, await instanceSummary(node));
      }
      case "instance.swap": {
        const instance = await nodeById(String(a.nodeId));
        const component = await nodeById(String(a.componentId));
        if (instance.type !== "INSTANCE" || component.type !== "COMPONENT") throw new Error("invalid_instance_swap");
        instance.swapComponent(component);
        return ok(request.id, await instanceSummary(instance), true);
      }
      case "instance.detach": {
        const instance = await nodeById(String(a.nodeId));
        if (instance.type !== "INSTANCE") throw new Error("not_instance");
        return ok(request.id, summarize(instance.detachInstance()), true);
      }

      case "variable.collection.list":
        return ok(request.id, (await figma.variables.getLocalVariableCollectionsAsync()).slice(0, MAX_RESULTS).map(c => ({id: c.id, name: c.name, modes: c.modes, defaultModeId: c.defaultModeId, variableIds: c.variableIds})));
      case "variable.list":
        return ok(request.id, (await figma.variables.getLocalVariablesAsync()).slice(0, MAX_RESULTS).map(v => ({id: v.id, name: v.name, resolvedType: v.resolvedType, valuesByMode: v.valuesByMode})));
      case "variable.collection.create": {
        const collection = figma.variables.createVariableCollection(String(a.name));
        return ok(request.id, {id: collection.id, name: collection.name, modes: collection.modes, defaultModeId: collection.defaultModeId}, true);
      }
      case "variable.create": {
        const collection = await figma.variables.getVariableCollectionByIdAsync(String(a.collectionId));
        if (!collection) throw new Error("collection_not_found");
        const variable = figma.variables.createVariable(String(a.name), collection, a.resolvedType as VariableResolvedDataType);
        return ok(request.id, {id: variable.id, name: variable.name, resolvedType: variable.resolvedType}, true);
      }
      case "variable.set_value": {
        const variable = await figma.variables.getVariableByIdAsync(String(a.variableId));
        if (!variable) throw new Error("variable_not_found");
        variable.setValueForMode(String(a.modeId), a.value as VariableValue);
        return ok(request.id, {id: variable.id, modeId: String(a.modeId), updated: true}, true);
      }
      case "variable.set_alias": {
        const variable = await figma.variables.getVariableByIdAsync(String(a.variableId));
        const target = await figma.variables.getVariableByIdAsync(String(a.aliasVariableId));
        if (!variable || !target) throw new Error("variable_not_found");
        variable.setValueForMode(String(a.modeId), figma.variables.createVariableAlias(target));
        return ok(request.id, {id: variable.id, modeId: String(a.modeId), aliasVariableId: target.id}, true);
      }
      case "variable.bind": {
        const node = asScene(await nodeById(String(a.nodeId))) as any;
        const variable = await figma.variables.getVariableByIdAsync(String(a.variableId));
        if (!variable || typeof node.setBoundVariable !== "function") throw new Error("binding_unavailable");
        node.setBoundVariable(String(a.field) as VariableBindableNodeField, variable);
        return ok(request.id, {bound: true, nodeId: node.id, field: String(a.field), variableId: variable.id}, true);
      }
      case "mode.list": {
        const collection = await figma.variables.getVariableCollectionByIdAsync(String(a.collectionId));
        if (!collection) throw new Error("collection_not_found");
        return ok(request.id, collection.modes);
      }
      case "mode.create": {
        const collection = await figma.variables.getVariableCollectionByIdAsync(String(a.collectionId));
        if (!collection) throw new Error("collection_not_found");
        const modeId = collection.addMode(String(a.name).slice(0, 256));
        return ok(request.id, {modeId, name: String(a.name).slice(0, 256)}, true);
      }

      case "design_system.extract":
        return ok(request.id, await extractDesignSystem());

      case "snapshot.page": {
        const page = a.pageId ? await pageById(String(a.pageId)) : figma.currentPage;
        return ok(request.id, await tree(page));
      }
      case "snapshot.subtree":
        return ok(request.id, await tree(await nodeById(String(a.nodeId))));
      case "snapshot.selection":
        return ok(request.id, figma.currentPage.selection.slice(0, MAX_RESULTS).map(summarize));
      case "snapshot.design_system":
        return ok(request.id, await extractDesignSystem());

      case "prototype.reaction.list": {
        const node = asScene(await nodeById(String(a.nodeId))) as SceneNode & ReactionMixin;
        return ok(request.id, node.reactions);
      }
      case "prototype.reaction.set": {
        const node = asScene(await nodeById(String(a.nodeId))) as SceneNode & ReactionMixin;
        await node.setReactionsAsync(a.reactions as Reaction[]);
        return ok(request.id, {count: node.reactions.length}, true);
      }
      case "prototype.reaction.add": {
        const node = asScene(await nodeById(String(a.nodeId))) as SceneNode & ReactionMixin;
        if (node.reactions.length >= 64) throw new Error("reaction_limit");
        await node.setReactionsAsync([...node.reactions, a.reaction as Reaction]);
        return ok(request.id, {count: node.reactions.length}, true);
      }
      case "prototype.reaction.remove": {
        const node = asScene(await nodeById(String(a.nodeId))) as SceneNode & ReactionMixin;
        const index = Number(a.index);
        if (!Number.isInteger(index) || index < 0 || index >= node.reactions.length) throw new Error("reaction_index");
        const reactions = [...node.reactions];
        reactions.splice(index, 1);
        await node.setReactionsAsync(reactions);
        return ok(request.id, {count: node.reactions.length}, true);
      }
      case "prototype.reaction.clear": {
        const node = asScene(await nodeById(String(a.nodeId))) as SceneNode & ReactionMixin;
        await node.setReactionsAsync([]);
        return ok(request.id, {count: 0}, true);
      }
      case "prototype.flow.list":
        return ok(request.id, figma.currentPage.flowStartingPoints.slice(0, MAX_RESULTS));

      case "motion.styles.list":
        return ok(request.id, figma.motion.figmaAnimationStyles());
      case "motion.node.inspect": {
        const node = asScene(await nodeById(String(a.nodeId))) as any;
        return ok(request.id, {animationStyles: node.animationStyles, manualKeyframeTracks: node.manualKeyframeTracks, animations: node.animations, timelines: node.timelines});
      }
      case "motion.keyframes.list": {
        const node = asScene(await nodeById(String(a.nodeId))) as any;
        return ok(request.id, node.manualKeyframeTracks ?? []);
      }
      case "motion.timelines.list": {
        const node = asScene(await nodeById(String(a.nodeId))) as any;
        return ok(request.id, node.timelines ?? []);
      }
      case "motion.spring.normalize":
        return ok(request.id, {bounce: figma.motion.physicalSpringToNormalized({mass: Number(a.mass), stiffness: Number(a.stiffness), damping: Number(a.damping)})});
      case "motion.style.apply": {
        const node = asScene(await nodeById(String(a.nodeId))) as any;
        node.applyAnimationStyle(String(a.styleId), {duration: Number(a.duration), timelineOffset: Number(a.timelineOffset ?? 0), props: a.props});
        return ok(request.id, {applied: true}, true);
      }
      case "motion.style.remove": {
        const node = asScene(await nodeById(String(a.nodeId))) as any;
        node.removeAnimationStyle(String(a.styleId));
        return ok(request.id, {removed: true}, true);
      }
      case "motion.keyframe.apply": {
        const keyframes = a.track?.keyframes;
        if (!Array.isArray(keyframes) || keyframes.length > MAX_KEYFRAMES) throw new Error("keyframe_limit");
        const node = asScene(await nodeById(String(a.nodeId))) as any;
        node.applyManualKeyframeTrack(a.field, a.track);
        return ok(request.id, {applied: true}, true);
      }
      case "motion.keyframe.remove": {
        const node = asScene(await nodeById(String(a.nodeId))) as any;
        node.removeManualKeyframeTrack(a.field);
        return ok(request.id, {removed: true}, true);
      }
      case "motion.timeline.set_duration": {
        const duration = Number(a.duration);
        if (!Number.isFinite(duration) || duration <= 0 || duration > 3600) throw new Error("invalid_duration");
        const node = asScene(await nodeById(String(a.nodeId))) as any;
        node.setTimelineDuration(String(a.timelineId), duration);
        return ok(request.id, {duration}, true);
      }

      case "figjam.sticky.create": {
        if (figma.editorType !== "figjam") throw new Error("figjam_only");
        const node = figma.createSticky();
        if (typeof a.name === "string") node.name = a.name.slice(0, 256);
        return ok(request.id, summarize(node), true);
      }
      case "figjam.shape.create": {
        if (figma.editorType !== "figjam") throw new Error("figjam_only");
        const node = figma.createShapeWithText();
        if (typeof a.name === "string") node.name = a.name.slice(0, 256);
        return ok(request.id, summarize(node), true);
      }
      case "figjam.connector.create": {
        if (figma.editorType !== "figjam") throw new Error("figjam_only");
        const connector = figma.createConnector();
        const from = asScene(await nodeById(String(a.from)));
        const to = asScene(await nodeById(String(a.to)));
        connector.connectorStart = {endpointNodeId: from.id, magnet: "AUTO"};
        connector.connectorEnd = {endpointNodeId: to.id, magnet: "AUTO"};
        return ok(request.id, summarize(connector), true);
      }
      case "figjam.section.create": {
        if (figma.editorType !== "figjam") throw new Error("figjam_only");
        const node = figma.createSection();
        if (typeof a.name === "string") node.name = a.name.slice(0, 256);
        return ok(request.id, summarize(node), true);
      }
      case "figjam.code_block.create": {
        if (figma.editorType !== "figjam") throw new Error("figjam_only");
        const node = figma.createCodeBlock();
        node.code = String(a.code ?? "").slice(0, 65_536);
        return ok(request.id, summarize(node), true);
      }

      case "dev.css": {
        const node = asScene(await nodeById(String(a.nodeId))) as any;
        if (typeof node.getCSSAsync !== "function") throw new Error("css_unavailable");
        return ok(request.id, await node.getCSSAsync());
      }

      default: {
        const extra = await handleSemanticComplete(request, a);
        if (extra) return extra;
        const more = await handleSemanticMore(request, a);
        if (more) return more;
        const semanticExport = await handleSemanticExports(request, a);
        if (semanticExport) return semanticExport;
        const semanticProperty = await handleSemanticProperties(request, a);
        if (semanticProperty) return semanticProperty;
        const admin = await handleSemanticAdmin(request, a);
        return admin ?? fail(request.id, "unsupported", "operation not implemented by plugin build");
      }
    }
  } catch (error) {
    return fail(request.id, "plugin_error", error instanceof Error ? error.message : "plugin failure");
  }
}

function startBridgeRuntime() {
  figma.showUI(__html__, {width: 360, height: 280, themeColors: true});
  figma.ui.onmessage = async (message: {type: string; request?: BridgeRequest}) => {
    if (message.type === "bridge-request" && message.request) {
      figma.ui.postMessage({type: "bridge-response", response: await handle(message.request)});
    }
    if (message.type === "bridge-status") {
      figma.ui.postMessage({type: "document-context", editorType: figma.editorType, documentId: figma.root.id, pageId: figma.currentPage.id, revision});
    }
  };
  figma.on("selectionchange", () => {
    revision++;
    figma.ui.postMessage({type: "event", kind: "selectionchange", revision});
  });
  figma.on("currentpagechange", () => {
    revision++;
    figma.ui.postMessage({type: "event", kind: "currentpagechange", revision});
  });
}

async function enableRemoteDocumentChangeTracking() {
  try {
    // dynamic-page plugins must load all pages before subscribing to documentchange.
    // Only REMOTE changes advance this revision: our own writes already advance it
    // synchronously in handle(), so counting LOCAL events again would create races.
    await figma.loadAllPagesAsync();
    figma.on("documentchange", (event: DocumentChangeEvent) => {
      const remote = event.documentChanges.filter(change => change.origin === "REMOTE");
      if (remote.length === 0) return;
      revision++;
      figma.ui.postMessage({
        type: "event",
        kind: "documentchange",
        revision,
        payload: {
          origin: "REMOTE",
          count: Math.min(remote.length, 256),
          changes: remote.slice(0, 64).map(change => ({id: change.id, type: change.type})),
          truncated: remote.length > 64,
        },
      });
    });
  } catch {
    figma.ui.postMessage({
      type: "event",
      kind: "documentchange_unavailable",
      revision,
      payload: {},
    });
  }
}

const automaticTextReviewMode = figma.mode === "textreview" || figma.command === "textreview";
if (automaticTextReviewMode) {
  figma.on("textreview", () => []);
} else {
  if (figma.editorType === "dev" && figma.mode === "codegen") {
    figma.codegen.on("generate", ({node}) => [{
      title: "Semwright semantic node",
      language: "JSON",
      code: JSON.stringify(summarize(node), null, 2),
    }]);
  }
  startBridgeRuntime();
  void enableRemoteDocumentChangeTracking();
}
