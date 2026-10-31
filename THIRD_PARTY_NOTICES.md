# Third-party notices

The Granny 2 (.gr2) model support ports code from these MIT-licensed projects.

## Rasetsuu/blendergranny

The Oodle0 section decompressor (`crates/ro-formats/src/oodle0.rs`) is a port of
`io_scene_gr2/gr2/decompress/oodle0.py` from https://github.com/Rasetsuu/blendergranny.

## magcius/noclip.website

Reading the Granny type tree, meshes, skeletons, textures and curves
(`crates/ro-formats/src/gr2.rs`) follows `src/RagnarokOnline/granny.ts` from
https://github.com/magcius/noclip.website.

## granny-ro-js

The wavelet texture decoder (`crates/ro-formats/src/igc.rs`) is a port of
`dist/granny-ro-igc.js` from granny-ro-js 1.5.0 (https://www.npmjs.com/package/granny-ro-js), and
the curve sampling and skinning in `crates/ro-render/src/model3d.rs` follow it.

## Licences

blendergranny: Copyright (c) 2026 ciupix and contributors.
noclip.website: Copyright (c) 2017-present noclip.website contributors.
granny-ro-js: Copyright (c) 2026 Stitchuuuu.

Each is released under the MIT License:

> Permission is hereby granted, free of charge, to any person obtaining a copy of this software
> and associated documentation files (the "Software"), to deal in the Software without
> restriction, including without limitation the rights to use, copy, modify, merge, publish,
> distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
> Software is furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all copies or
> substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
> BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
> NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
> DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
