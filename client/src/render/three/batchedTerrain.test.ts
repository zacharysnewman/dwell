import { MeshBasicMaterial } from 'three';
import { describe, expect, it } from 'vitest';
import type { FlatMesh, SectionMeshes } from '../../mesh/lodMesher';
import type { ChunkMeshes, MeshArrays } from '../../mesh/mesher';
import type { ChunkCoord } from '../../protocol/messages';
import { BatchedTerrain } from './batchedTerrain';

function quad(): Omit<MeshArrays, 'uvs' | 'tiles'> {
  return {
    positions: new Float32Array(12),
    normals: new Float32Array(12),
    colors: new Float32Array(12),
    indices: new Uint32Array([0, 1, 2, 0, 2, 3]),
  };
}
const none = (): MeshArrays => ({
  positions: new Float32Array(0),
  normals: new Float32Array(0),
  colors: new Float32Array(0),
  uvs: new Float32Array(0),
  tiles: new Float32Array(0),
  indices: new Uint32Array(0),
});
const textured = (): MeshArrays => ({
  ...quad(),
  uvs: new Float32Array(8),
  tiles: new Float32Array(16),
});
const chunk = (water: boolean): ChunkMeshes => ({
  opaque: textured(),
  transparent: water ? textured() : none(),
});
const flat = (n: number): FlatMesh => (n > 0 ? quad() : { ...none() });
/** A section with a surface and skirts on the sides in `sides` (bit per face). */
function section(sides: number): SectionMeshes {
  return {
    opaque: flat(1),
    water: flat(0),
    skirts: [0, 1, 2, 3, 4, 5].map((f) => flat((sides >> f) & 1)),
  };
}

function terrain(): BatchedTerrain {
  const m = new MeshBasicMaterial();
  return new BatchedTerrain({ chunk: m, chunkWater: m, lod: m });
}

describe('batched terrain (?batch=1, §6.6)', () => {
  it('holds every chunk and section in three batches, replacing and removing them', () => {
    const t = terrain();
    expect(t.meshes).toHaveLength(3);
    for (let i = 0; i < 100; i++)
      t.setChunk(`c${String(i)}`, [i * 32, 0, 0], [i, 0, 0], chunk(i % 4 === 0));
    for (let i = 0; i < 50; i++) t.setLodSection(i, [0, 0, i * 64], 2, section(0b111111));
    expect(t.counts).toEqual({ chunks: 100, chunkWater: 25, lod: 50 * 7 });
    // A chunk remeshed (an edit) replaces its members; an unloaded one removes them.
    t.setChunk('c0', [0, 0, 0], [0, 0, 0], chunk(false));
    t.setChunk('c1', [32, 0, 0], [1, 0, 0], null);
    t.setLodSection(3, [0, 0, 0], 2, null);
    expect(t.counts).toEqual({ chunks: 99, chunkWater: 24, lod: 49 * 7 });
  });

  it('shows the chunks the LOD does not cover, and the sections listed with their skirts', () => {
    const t = terrain();
    for (let i = 0; i < 4; i++) t.setChunk(`c${String(i)}`, [i * 32, 0, 0], [i, 0, 0], chunk(true));
    // Sections 1 and 2 have skirts on +X (bit 0) and -Y (bit 3) only.
    t.setLodSection(1, [0, 0, 0], 4, section(0b001001));
    t.setLodSection(2, [0, 0, 128], 4, section(0b001001));
    // Chunks show until the LOD hides them; sections are hidden until listed.
    t.update(null, new Map());
    expect(t.visible()).toEqual({ chunks: 4, sections: 0, skirts: 0 });
    const even = (c: ChunkCoord): boolean => c[0] % 2 === 0;
    // Section 1 with its +X and -Y skirts (and -X: it has none), section 2 with none.
    t.update(
      even,
      new Map([
        [1, 0b001011],
        [2, 0],
      ]),
    );
    expect(t.visible()).toEqual({ chunks: 2, sections: 2, skirts: 2 });
    t.update(even, new Map([[2, 0b001000]]));
    expect(t.visible()).toEqual({ chunks: 2, sections: 1, skirts: 1 });
  });
});
