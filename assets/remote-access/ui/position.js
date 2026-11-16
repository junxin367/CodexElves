// 通用浮层定位规则：普通按钮菜单、Select 弹层与模型菜单共用同一水平对齐方式。

export function menuHorizontalLeft(anchor, menuWidth, viewport, padding = 8) {
  // 用户要求通用菜单按可视区域左 65% / 右 35% 展开：按钮中心到达 65% 分界时才右对齐、向左展开。
  // 0.65 只表示方向分界；缩放/平移后的 left、原 8px 边缘留白、菜单宽度和上下定位均保留。
  // Select、Open in 和模型菜单共用本函数；会话主菜单的区域内 20px 定位等例外见 docs/product-standards.md。
  const boundary = viewport.left + viewport.width * 0.65;
  const alignLeft = (anchor.left + anchor.right) / 2 < boundary;
  const preferred = alignLeft ? anchor.left : anchor.right - menuWidth;
  return Math.max(viewport.left + padding,
    Math.min(preferred, viewport.left + viewport.width - menuWidth - padding));
}
