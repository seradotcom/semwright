---
name: TIDELING
description: An original native Godot reef survival prototype with a quiet overlay interface.
colors:
  reef-water: "#083c50"
  reef-fog: "#0d4257"
  reef-ambient: "#648a94"
  surface-sun: "#ffe1ac"
  reef-fill: "#8fe0d6"
  foreground-frond: "#073f42"
  suspended-mote: "#b5e2d6"
  warm-text: "#f4ecd9"
  quiet-text: "#b7dad6"
  sand-action: "#efbf89"
  sand-action-hover: "#ffdda6"
  action-ink: "#183847"
  combo-gold: "#ffce89"
  burst-seaglass: "#a6dcd4"
  meal-mark: "#e0ffe7"
  hunter-mark: "#ffbf83"
  bar-track: "rgba(2%, 12%, 16%, 0.8)"
  label-shadow: "rgba(1.5%, 7%, 9%, 0.9)"
typography:
  display:
    fontFamily: "Fraunces"
    fontSize: "78px"
  ending:
    fontFamily: "Fraunces"
    fontSize: "52px"
  score:
    fontFamily: "Godot default theme font"
    fontSize: "28px"
  stage:
    fontFamily: "Godot default theme font"
    fontSize: "23px"
  introduction:
    fontFamily: "Godot default theme font"
    fontSize: "22px"
  action:
    fontFamily: "Godot default theme font"
    fontSize: "20px"
  body:
    fontFamily: "Godot default theme font"
    fontSize: "18px"
  controls:
    fontFamily: "Godot default theme font"
    fontSize: "16px"
  caption:
    fontFamily: "Godot default theme font"
    fontSize: "13px"
rounded:
  action-diagonal: "4px 0 4px 0"
spacing:
  hud-left: "44px"
  menu-left: "70px"
components:
  dive-action:
    backgroundColor: "{colors.sand-action}"
    textColor: "{colors.action-ink}"
    typography: "{typography.action}"
    rounded: "{rounded.action-diagonal}"
    width: "210px"
    height: "52px"
  dive-action-hover:
    backgroundColor: "{colors.sand-action-hover}"
    textColor: "{colors.action-ink}"
  dive-action-pressed:
    backgroundColor: "{colors.sand-action-hover}"
    textColor: "{colors.action-ink}"
  growth-track:
    backgroundColor: "{colors.bar-track}"
    width: "270px"
    height: "5px"
  growth-fill:
    backgroundColor: "{colors.sand-action}"
  burst-track:
    backgroundColor: "{colors.bar-track}"
    width: "150px"
    height: "5px"
  burst-fill:
    backgroundColor: "{colors.burst-seaglass}"
---

# Design System: TIDELING

## Overview

**Creative North Star: "A small life in a vast reef"**

The original modeled fish and reef carry the experience. A warm hero moves through cool water, framed by layered coral, rock and kelp. The interface is a sparse native Godot CanvasLayer over the living scene; opening and ending copy use an editorial serif while play information stays compact.

This document records the implemented sources: `project/scripts/game.gd`, `reef.gd`, `food_marks.gd`, the menu and HUD shaders, and `project/project.godot`. Final visual/interface review of the six `hosted-007` `valid1280x800` states (title, gameplay, hierarchy, stage two, stage three and ending) returned SHIP with no material fixes required. The hero is unobstructed, growth stages are distinct, and HUD, instructions and actions are legible. The review found a cohesive small demo at its current visual ceiling; its disposition is limited to those captured states and is not commercial-quality certification. These staged fixtures do not establish organic progression, animation timing, human movement feel, complete accessibility or Semwright driver proof. The source behavior documented below does not claim a later hosted run or publication has passed. Asset silhouette and material intent remain in `design/ART_DIRECTION.md`; their directional palette is not a substitute for the runtime UI tokens above.

**Key Characteristics:**

- Warm fish and action accents against cool, layered water.
- A live reef behind the opening and ending overlay.
- Small edge-aligned HUD labels and thin progress tracks.
- Food-chain marks distinguished by geometry as well as color.
- Gentle camera movement and a reduced-effects option with limited scope.

## Colors

### Primary

Sand action colors connect the Dive button to the growth meter. The lighter action color is shared by hover and pressed states. Dark action ink keeps the warm button label distinct.

### Secondary

Burst sea glass identifies recharge; combo gold highlights an active multiplier. Meal marks use pale green paired arcs, while hunter marks use apricot outlined diamonds. Eligibility comes from gameplay rules, size, behavior and mark geometry together.

### Neutral

Warm text carries primary labels; quiet text carries controls, the introduction and timer. The track and label-shadow colors are translucent Godot values preserved as percentage RGB in the frontmatter. Water, fog, ambient light, sunlight, fill light, foreground fronds and motes name their actual scene roles; rendered appearance also depends on lighting and filmic tonemapping.

**The Source Color Rule.** Treat runtime UI colors and authored asset palette colors as separate roles. Do not replace an observed button or atmosphere value with a similar art-direction swatch.

## Typography

The title loads the bundled Fraunces `project/assets/title.ttf` when present. Opening and ending titles use the display and ending sizes respectively. If that resource is absent, the code inherits the Godot default font. Other controls use the inherited Godot theme font; the project does not pin a separate body family, weight, line-height or tracking override.

The score is the largest play readout. Stage and combo share the stage size. Introductory copy uses the introduction size; the Dive button uses action; gameplay hints use body; control instructions and timer use controls; burst instructions use caption. Sound and reduced-effects controls retain inherited native theme sizing.

## Layout

The viewport is authored at 1280 × 800 with `canvas_items` stretch. Measurements are logical canvas pixels. Controls use explicit positions rather than a responsive container grid; alternate aspect-ratio reflow has not been implemented or verified.

The opening title starts at (66, 180), introductory text at (70, 295), Dive at (70, 418), controls at (70, 509), and reduced-effects toggle at (62, 590). The scene remains visible behind a left-weighted shader shade. The ending reuses this overlay and button.

During play, stage starts at (44, 32), growth at (44, 71), score at (1060, 28), timer at (1135, 67), and combo at (595, 36). Burst instructions start at (44, 726), recharge at (44, 753), and hint text at (320, 724). Sound sits at (1080, 734) with a 150 × 36 assigned size. Growth and recharge track dimensions are defined above. HUD edge shading covers the full authored canvas.

The orthographic camera begins at size 23. Active target size is 18.5, 20.8 or 23.1 across three stages, plus the growth pulse contribution. Camera follow is damped. The reef places three background layers behind the swimming plane, with near-edge fronds framing the lagoon; unobstructed gameplay is the intent, with the central foliage obstruction resolved in the hosted juvenile capture.

## Elevation & Depth

Depth comes from original 3D assets, directional shadows, fog, material lighting, layered silhouettes and an orthographic camera. Native overlay labels use a dark shadow offset by (1, 2) pixels. Menu shading fades horizontally; HUD shading gathers at the top and bottom. There are no custom elevated UI cards or panel shadows.

Growth sets a pulse to 1 and decays it over approximately one second, with an added camera-size contribution of up to 0.8 and a pale screen tint of up to 0.07 alpha. The previous 0.9-second direction is not the implemented timing. Reduced effects suppress this tint and spawned effect particles, including dash particles; camera breathing, frond motion and ambient motes continue. This toggle is not documented as comprehensive reduced motion.

## Shapes

The Dive button rounds only its top-left and bottom-right corners; the other two corners stay square. Progress tracks are flat rectangles without percentage text. Nearby edible fish receive two opposed arcs, with a dark outer stroke (4 pixels) and pale inner stroke (2.2 pixels), at a radius clamped between 10 and 28 pixels. Hunters receive an outlined diamond (2-pixel stroke). Both marks appear only during active play and within five world units of the player.

The hero's authored leaf-shaped dorsal crest, crescent tail, lucid eyes and split translucent fins belong to the asset direction. Threat silhouette and behavior must remain readable alongside eligibility marks.

## Components

### Dive action and ending overlay

A single warm native Button starts play; it becomes “Dive again” at the ending. Hover and pressed share the lighter sand fill. All explicitly set text states use action ink. The code grabs keyboard focus on opening and ending; focus decoration itself comes from the inherited Godot theme and has not been independently specified here. Enter starts while inactive, and R retries after ending. Ending titles shrink to the ending typography role and show points, elapsed seconds and stage in the subtitle.

### Hero animation and retry

The opening hero plays `Idle_loop`. During play, ordinary swimming and idle states select `Idle_loop` below a movement speed of 0.2 and `Swim_loop` otherwise; reversing horizontal input triggers `Turn`. These are source-observed state choices, not a certification of animation timing or human movement feel.

Starting or retrying resets the spawn clock and growth pulse, queues existing transient effect nodes for removal, and clears their tracking list. A new dive therefore begins without the prior run's pending spawn interval, growth pulse or transient particles. This reset behavior is documented from source; the captured visual states do not independently verify it.

### Growth and burst meters

Native ProgressBars use translucent tracks and distinct warm-growth or cool-burst fills. Values range from zero to one; numeric percentages are hidden. The stage label reads Juvenile, Explorer or Reefkeeper. A combo appears only when the multiplier exceeds one.

### Food-chain marks

A mouse-ignoring Control projects nearby fish positions into screen space and draws the arcs or diamond from current gameplay eligibility. The marks do not introduce a selectable target or a clickable interface.

### Sound and reduced effects

The native sound Button toggles the master bus and updates its label; M provides the same action. A native CheckButton on the menu controls reduced effects. These two controls retain the engine theme rather than inheriting the bespoke Dive style. Do not infer custom focus, hover or disabled styles for them from this document.

### Play hints and pause

The initial food-chain hint clears after eight elapsed seconds. Escape or controller Start toggles pause during play and supplies a resume hint. Pause is expressed through that label, without a new panel or modal. Reef ambient motion and camera processing are not fully stopped by the current pause implementation.

## Do's and Don'ts

### Do:

- Do keep the original fish and reef as the main visual content.
- Do preserve geometry and gameplay eligibility alongside food-chain colors.
- Do use runtime source values for UI tokens and identify inherited native theme behavior.
- Do keep authoring and proof controls outside the shipping game interface.
- Do describe this implementation as a prototype until its visual and functional gates have observed evidence.

### Don'ts:

- Don't turn growth into a level-up dialog or the reef into a card grid.
- Don't claim responsive reflow, comprehensive reduced motion or approved golden visuals from source inspection.
- Don't treat this documentation or its native sidecar as Semwright driver proof.
- Don't replace original fish silhouettes with borrowed character designs.
