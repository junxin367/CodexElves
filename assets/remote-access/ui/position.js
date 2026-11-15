// 通用浮层定位规则：按钮菜单、Select 弹层、模型菜单与会话菜单共用同一水平对齐方式。

export function menuHorizontalLeft(anchor, menuWidth, viewport, padding = 8) {
  // 用户要求按钮菜单按可视区域中线选择方向：按钮中心在左半边时左对齐，否则右对齐（中线归右）。
  // 可视区域包含缩放/平移后的 left；两种方向均保留原来的 8px 边缘留白，不改变菜单宽度或上下定位。
  const alignLeft = (anchor.left + anchor.right) / 2 < viewport.left + viewport.width / 2;
  const preferred = alignLeft ? anchor.left : anchor.right - menuWidth;
  return Math.max(viewport.left + padding,
    Math.min(preferred, viewport.left + viewport.width - menuWidth - padding));
}
