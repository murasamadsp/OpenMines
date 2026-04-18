/** Палитра как в `client/Assets/Scripts/MapViewer.cs` → `InitColorTable` + `RGBQ`. */
function rgbq(r: number, g: number, b: number): [number, number, number] {
  return [
    Math.round((r / 256) * 255),
    Math.round((g / 256) * 255),
    Math.round((b / 256) * 255),
  ];
}

function def(i: number): [number, number, number] {
  return [
    Math.round((i / 512) * 255),
    Math.round((i / 256) * 255),
    Math.round(0.01 * 255),
  ];
}

/** RGBA на клетку; индекс = тип клетки 0…255. */
export function buildMapPalette(): Uint8ClampedArray {
  const p = new Uint8ClampedArray(256 * 4);
  const set = (i: number, r: number, g: number, b: number, a = 255) => {
    const o = i * 4;
    p[o] = r;
    p[o + 1] = g;
    p[o + 2] = b;
    p[o + 3] = a;
  };

  for (let i = 0; i < 255; i++) {
    const [r, g, b] = def(i);
    set(i, r, g, b);
  }

  set(0, 0, 0, 0, 128);
  set(1, 0, 0, 0, 128);

  const o = (t: number, rgb: [number, number, number]) => set(t, rgb[0], rgb[1], rgb[2]);
  o(32, rgbq(0, 0, 0));
  o(33, rgbq(15, 11, 3));
  o(34, rgbq(29, 25, 18));
  o(35, rgbq(68, 68, 68));
  o(36, rgbq(85, 68, 34));
  o(37, rgbq(68, 0, 0));
  o(38, rgbq(51, 68, 0));
  o(40, rgbq(255, 97, 107));
  o(41, rgbq(255, 107, 97));
  o(42, rgbq(255, 107, 107));
  o(43, rgbq(255, 187, 251));
  o(44, rgbq(191, 241, 251));
  o(45, rgbq(207, 203, 241));
  o(48, rgbq(255, 255, 255));
  o(49, rgbq(101, 150, 126));
  o(50, rgbq(101, 255, 255));
  o(51, rgbq(255, 51, 51));
  o(52, rgbq(255, 101, 255));
  o(53, rgbq(34, 101, 255));
  o(54, rgbq(238, 254, 255));
  o(55, rgbq(238, 254, 255));
  o(56, rgbq(225, 254, 255));
  o(57, rgbq(226, 254, 255));
  o(58, rgbq(227, 254, 255));
  o(59, rgbq(228, 254, 255));
  o(60, rgbq(204, 204, 204));
  o(61, rgbq(221, 221, 221));
  o(62, rgbq(255, 204, 204));
  o(63, rgbq(255, 221, 221));
  o(64, rgbq(170, 170, 170));
  o(65, rgbq(187, 187, 187));
  o(66, rgbq(184, 153, 51));
  o(67, rgbq(184, 136, 187));
  o(68, rgbq(119, 68, 68));
  o(69, rgbq(34, 68, 153));
  o(70, rgbq(243, 241, 152));
  o(71, rgbq(71, 215, 100));
  o(72, rgbq(101, 134, 247));
  o(73, rgbq(247, 82, 67));
  o(74, rgbq(132, 238, 247));
  o(75, rgbq(255, 135, 231));
  o(82, rgbq(17, 102, 102));
  o(83, rgbq(50, 135, 152));
  o(86, rgbq(184, 255, 17));
  o(90, rgbq(238, 238, 238));
  o(91, rgbq(255, 90, 0));
  o(92, rgbq(193, 187, 187));
  o(93, rgbq(187, 193, 187));
  o(94, rgbq(187, 187, 193));
  o(95, rgbq(184, 255, 34));
  o(96, rgbq(184, 255, 68));
  o(97, rgbq(112, 160, 183));
  o(98, rgbq(112, 187, 207));
  o(99, rgbq(219, 209, 125));
  o(100, rgbq(181, 168, 57));
  o(101, rgbq(76, 191, 0));
  o(102, rgbq(208, 206, 0));
  o(103, rgbq(133, 81, 166));
  o(104, rgbq(153, 153, 136));
  o(105, rgbq(198, 0, 0));
  o(106, rgbq(136, 136, 136));
  o(107, rgbq(8, 215, 100));
  o(108, rgbq(255, 0, 0));
  o(109, rgbq(0, 0, 255));
  o(110, rgbq(255, 0, 255));
  o(111, rgbq(238, 238, 255));
  o(112, rgbq(0, 255, 255));
  o(113, rgbq(211, 159, 166));
  o(114, rgbq(119, 119, 119));
  o(115, rgbq(56, 118, 65));
  o(116, rgbq(17, 17, 255));
  o(117, rgbq(170, 119, 119));
  o(118, rgbq(100, 98, 21));
  o(119, rgbq(170, 255, 255));
  o(120, rgbq(227, 191, 120));
  o(121, rgbq(163, 136, 72));
  o(122, rgbq(51, 153, 120));

  {
    const [r, g, b] = def(255);
    set(255, r, g, b);
  }

  return p;
}
