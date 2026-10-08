---
version: alpha
name: ClashBar Windows
description: A compact, local-first Windows proxy control panel.
colors:
  primary: "#075db6"
  canvas: "#f4f6f9"
  surface: "#ffffff"
  ink: "#182434"
  muted: "#596778"
  border: "#cbd3de"
  accent: "#075db6"
  success: "#176640"
  danger: "#b52230"
typography:
  display:
    fontFamily: '"Segoe UI Variable Display", "Segoe UI", sans-serif'
    fontSize: "19px"
  sans:
    fontFamily: '"Segoe UI Variable Text", "Segoe UI", "Microsoft YaHei UI", sans-serif'
    fontSize: "14px"
  mono:
    fontFamily: '"Cascadia Code", Consolas, monospace'
    fontSize: "11px"
rounded:
  DEFAULT: "5px"
  panel: "8px"
spacing:
  rhythm: "8px"
  control-height: "34px"
  page-max: "1020px"
components:
  button: { height: "34px", backgroundColor: "{colors.primary}", textColor: "{colors.surface}" }
  dialog: { width: "440px", backgroundColor: "{colors.surface}", textColor: "{colors.ink}" }
  table: { height: "470px", backgroundColor: "{colors.surface}", textColor: "{colors.ink}" }
  canvas: { backgroundColor: "{colors.canvas}", textColor: "{colors.ink}" }
  caption: { textColor: "{colors.muted}" }
  divider: { backgroundColor: "{colors.border}" }
  selected-tab: { textColor: "{colors.accent}" }
  running-status: { textColor: "{colors.success}" }
  error-message: { textColor: "{colors.danger}" }
---

# ClashBar Windows Design System

## Overview

### Creative North Star

A Windows tray utility: compact controls, one persistent service strip, and quiet operational data. Preserve ClashBar's original menu-panel job rather than adopting a marketing dashboard. The signature is a thin blue service rail above the current configuration, paired with the compact C mark.

### Product context and register

The user is adapting the existing SwiftUI/AppKit mihomo client to Windows. The audience manages a local core, proxy routing and connections. The register is a product tool. Simplified Chinese is the initial UI locale; configuration and node names remain verbatim. There is no inferred market-specific business policy. Settings are used occasionally; status, nodes and connections are frequent tasks. Avoid oversized metrics, gradients, decorative charts and promotional copy.

Runtime tokens in `src/style.css` are canonical; this document mirrors their accepted light-theme values. Components consume CSS variables directly, with no independent theme adapter. Update both in the same change. The dark media query changes semantic tokens, preserving geometry.

## Colors

Cool gray canvas, white surface and a Windows blue accent separate controls from live data. Green is used only with a running label. Red marks an error or the final interrupt-connection confirmation. Dark mode follows the operating system; forced-colors follows system colors. Global scrollbar tokens define thumb, track, hover and active states.

## Typography

Segoe UI Display is limited to the product name; Segoe UI Text and Microsoft YaHei UI handle Chinese control copy. Cascadia Code/Consolas handles paths, log lines, addresses and numeric measurements. Body text is 14px/1.5; dense data is 12px. Full values wrap; important content must not depend on a hover tooltip.

## Layout

A natural document scroller owns each tab. The main content is at most 1020px; below 580px controls wrap and port fields stack. Only tables and the log region have bounded internal scrolling. Settings always retain natural height. The service strip and tabs remain mounted across refreshes; help and validation space is reserved. A 34px control height suits a pointer-first desktop utility while exceeding the 24px accessibility minimum.

## Elevation & Depth

Borders and subtle surfaces carry hierarchy. Only the modal receives a shadow. There is no blurred chrome or decorative card stacking.

## Shapes

5px controls and 8px panels keep the Windows feel. The asymmetric compact C mark is the only expressive shape. Status dots always have text.

## Components

Buttons share native semantics, a visible focus ring, hover, active, disabled and busy states. One blue primary marks the service action; dangerous actions are quiet in tables and prominent only in their confirmation. Busy labels preserve width. Native select popup geometry and keyboard behavior belong to Windows/WebView2; no custom combobox is implied. Native app-owned HTML dialogs use `showModal()` for focus containment and inert background. Searches include a clear action. Errors are persistent and live-announced; acknowledgements use one stable status region.

Motion is limited to busy feedback, disabled for reduced-motion preferences. No images, remote fonts or decorative icon libraries are loaded. Chinese action labels name the concrete operation. Byte units use IEC notation and numbers use `zh-CN` formatting.

## Do's and Don'ts

- Keep service state, configuration and system proxy visible together.
- Preserve focus, draft values and table positions while refreshing.
- Do not show fabricated traffic, example nodes or success before backend acknowledgement.
- Do not make a browser preview appear capable of changing desktop settings.
