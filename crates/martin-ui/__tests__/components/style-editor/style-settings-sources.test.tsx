import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  martinGlyphsUrl,
  martinSpriteUrl,
  type StyleResources,
  StyleResourcesProvider,
} from '@/components/style-editor/style-resources';
import { ResourcesSection } from '@/components/style-editor/style-settings/resources-section';
import { SourcesSection } from '@/components/style-editor/style-settings/sources-section';

vi.mock('lucide-react', async (importOriginal) => importOriginal());

let recorded: { label: string; style: StyleSpecification }[] = [];

function renderSection(
  Section: typeof ResourcesSection,
  style: StyleSpecification,
  resources: Partial<StyleResources> = {},
) {
  recorded = [];
  const apply = (label: string, recipe: (draft: StyleSpecification) => void) => {
    const draft = structuredClone(style);
    recipe(draft);
    recorded.push({ label, style: draft });
  };
  render(
    <StyleResourcesProvider value={resources}>
      <Section apply={apply} map={null} style={style} />
    </StyleResourcesProvider>,
  );
}

function stubFetch(handler: (url: string) => Response) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) => handler(url)),
  );
}

describe('ResourcesSection', () => {
  beforeEach(() => stubFetch(() => new Response('{}', { status: 200 })));
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it('writes the Martin glyphs URL and warns about fonts Martin does not serve', () => {
    renderSection(
      ResourcesSection,
      {
        glyphs: 'https://example.com/fonts/{fontstack}/{range}.pbf',
        layers: [
          {
            id: 'labels',
            layout: { 'text-font': ['Noto Sans Regular', 'Overpass Mono Regular'] },
            source: 'osm',
            type: 'symbol',
          },
        ],
        sources: { osm: { type: 'vector', url: 'https://example.com/osm' } },
        version: 8,
      },
      {
        fontsInStyle: ['Noto Sans Regular', 'Overpass Mono Regular'],
        martinFonts: ['Overpass Mono Regular'],
      },
    );

    fireEvent.click(screen.getByRole('button', { name: 'Use Martin fonts' }));

    expect(recorded.map((r) => [r.label, r.style.glyphs])).toEqual([
      ['Use Martin fonts', martinGlyphsUrl()],
    ]);
    expect(screen.getByText('Noto Sans Regular').tagName).toBe('SPAN');
    expect(
      screen.getByText(/Martin does not serve these fonts, so they would stop rendering/)
        .textContent,
    ).toMatchInlineSnapshot(`"Martin does not serve these fonts, so they would stop rendering."`);
  });

  it('names the placeholder a glyphs URL lacks', () => {
    renderSection(ResourcesSection, {
      glyphs: 'https://example.com/fonts/{fontstack}.pbf',
      layers: [],
      sources: {},
      version: 8,
    });

    expect(screen.getByText('{range}').textContent).toBe('{range}');
    expect(screen.queryByText('{fontstack}')).toBeNull();
  });

  it('reports that the first font of the style loads from the glyphs server', async () => {
    stubFetch((url) => {
      expect(url).toBe('https://glyphs-ok.example.com/Noto%20Sans%20Regular/0-255.pbf');
      return new Response('', { status: 200 });
    });
    renderSection(
      ResourcesSection,
      {
        glyphs: 'https://glyphs-ok.example.com/{fontstack}/{range}.pbf',
        layers: [],
        sources: {},
        version: 8,
      },
      { fontsInStyle: ['Noto Sans Regular'] },
    );

    expect((await screen.findByText('Loads Noto Sans Regular')).textContent).toBe(
      'Loads Noto Sans Regular',
    );
  });

  it('reports the HTTP status when the glyphs server fails', async () => {
    stubFetch(() => new Response('', { status: 404 }));
    renderSection(
      ResourcesSection,
      {
        glyphs: 'https://glyphs-404.example.com/{fontstack}/{range}.pbf',
        layers: [],
        sources: {},
        version: 8,
      },
      { fontsInStyle: ['Noto Sans Regular'] },
    );

    expect((await screen.findByText('Failed: 404')).textContent).toBe('Failed: 404');
  });

  it('turns a string sprite into the array form when a Martin sprite is added', () => {
    renderSection(
      ResourcesSection,
      {
        layers: [],
        sources: {},
        sprite: 'https://example.com/sprite',
        version: 8,
      },
      { martinSprites: ['src1'] },
    );

    fireEvent.click(screen.getByRole('button', { name: 'Add sprite src1' }));

    expect(recorded.map((r) => [r.label, r.style.sprite])).toEqual([
      [
        'Add sprite src1',
        [
          { id: 'default', url: 'https://example.com/sprite' },
          { id: 'src1', url: martinSpriteUrl('src1') },
        ],
      ],
    ]);
  });

  it('adds the SDF variant of a Martin sprite under its own id', () => {
    renderSection(
      ResourcesSection,
      { layers: [], sources: {}, sprite: 'https://example.com/sprite', version: 8 },
      { martinSprites: ['src1'] },
    );

    fireEvent.click(screen.getByRole('button', { name: 'Add SDF sprite src1' }));

    expect(recorded[0].style.sprite).toEqual([
      { id: 'default', url: 'https://example.com/sprite' },
      { id: 'src1-sdf', url: martinSpriteUrl('src1', true) },
    ]);
  });

  it('writes the string form back when one default sheet remains', () => {
    renderSection(ResourcesSection, {
      layers: [],
      sources: {},
      sprite: [
        { id: 'default', url: 'https://example.com/a' },
        { id: 'extra', url: 'https://example.com/b' },
      ],
      version: 8,
    });

    fireEvent.click(screen.getByRole('button', { name: 'Remove sheet extra' }));

    expect(recorded[0].style.sprite).toBe('https://example.com/a');
  });

  it('shows how many icons a sprite sheet holds', async () => {
    stubFetch((url) => {
      expect(url).toBe('https://sprite-ok.example.com/sprite.json');
      return new Response(JSON.stringify({ a: {}, b: {}, c: {} }), { status: 200 });
    });
    renderSection(ResourcesSection, {
      layers: [],
      sources: {},
      sprite: 'https://sprite-ok.example.com/sprite',
      version: 8,
    });

    expect((await screen.findByText('3 icons')).textContent).toBe('3 icons');
  });

  it('refuses a sheet id that is already used', () => {
    renderSection(ResourcesSection, {
      layers: [],
      sources: {},
      sprite: 'https://example.com/sprite',
      version: 8,
    });

    fireEvent.click(screen.getByRole('button', { name: 'Add sheet' }));
    fireEvent.change(screen.getByLabelText('Sheet id'), { target: { value: 'default' } });
    fireEvent.change(screen.getByLabelText('Sheet URL'), { target: { value: 'https://x/y' } });

    expect(screen.getByText('Id is already used').textContent).toBe('Id is already used');
    expect((screen.getByRole('button', { name: 'Add' }) as HTMLButtonElement).disabled).toBe(true);
  });
});

const sourcesStyle: StyleSpecification = {
  layers: [
    { id: 'water', source: 'openmaptiles', 'source-layer': 'water', type: 'fill' },
    { id: 'roads', source: 'openmaptiles', 'source-layer': 'road', type: 'line' },
    { id: 'bg', type: 'background' },
  ],
  sources: {
    openmaptiles: { maxzoom: 14, type: 'vector', url: 'https://tiles.example.com/planet' },
    other: { tiles: ['https://t.example.com/{z}/{x}/{y}.png'], type: 'raster' },
  },
  terrain: undefined,
  version: 8,
};

describe('SourcesSection', () => {
  beforeEach(() =>
    stubFetch(
      () =>
        new Response(
          JSON.stringify({
            maxzoom: 14,
            name: 'Planet',
            vector_layers: [{ id: 'a' }, { id: 'b' }],
          }),
          { status: 200 },
        ),
    ),
  );
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it('lists each source with its type, location and layer count', () => {
    renderSection(SourcesSection, sourcesStyle);

    expect(screen.getByText('used by 2 layers').textContent).toBe('used by 2 layers');
    expect(screen.getByText('used by 0 layers').textContent).toBe('used by 0 layers');
    expect(screen.getByText('https://tiles.example.com/planet').textContent).toBe(
      'https://tiles.example.com/planet',
    );
    expect(screen.getByText('inline tiles').textContent).toBe('inline tiles');
  });

  it('lists the layers that use a source when its count is clicked', () => {
    renderSection(SourcesSection, sourcesStyle);

    fireEvent.click(screen.getByText('used by 2 layers'));

    expect(screen.getByText('water').textContent).toBe('water');
    expect(screen.getByText('roads').textContent).toBe('roads');
  });

  it('renames a source and the layers that draw from it', () => {
    renderSection(SourcesSection, sourcesStyle);

    fireEvent.click(screen.getByRole('button', { name: 'Expand source openmaptiles' }));
    fireEvent.change(screen.getByLabelText('id'), { target: { value: 'omt' } });
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));

    expect(recorded.map((r) => r.label)).toEqual(['Rename source openmaptiles to omt']);
    expect(Object.keys(recorded[0].style.sources)).toEqual(['other', 'omt']);
    expect(recorded[0].style.layers.map((l) => ('source' in l ? l.source : undefined))).toEqual([
      'omt',
      'omt',
      undefined,
    ]);
  });

  it('refuses to rename a source to an id that is taken', () => {
    renderSection(SourcesSection, sourcesStyle);

    fireEvent.click(screen.getByRole('button', { name: 'Expand source openmaptiles' }));
    fireEvent.change(screen.getByLabelText('id'), { target: { value: 'other' } });
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));

    expect(recorded).toEqual([]);
    expect(screen.getByText('Id is empty or already used').textContent).toBe(
      'Id is empty or already used',
    );
  });

  it('removes a source together with the layers listed in the confirmation', () => {
    renderSection(SourcesSection, sourcesStyle);

    fireEvent.click(screen.getByRole('button', { name: 'Expand source openmaptiles' }));
    fireEvent.click(screen.getByRole('button', { name: 'Remove' }));

    expect(screen.getByText('2 layers are removed with it:').textContent).toBe(
      '2 layers are removed with it:',
    );

    fireEvent.click(screen.getByRole('button', { name: 'Remove source' }));

    expect(recorded.map((r) => r.label)).toEqual(['Remove source openmaptiles']);
    expect(Object.keys(recorded[0].style.sources)).toEqual(['other']);
    expect(recorded[0].style.layers.map((l) => l.id)).toEqual(['bg']);
  });

  it('shows the TileJSON summary of a url source', async () => {
    renderSection(SourcesSection, sourcesStyle);

    fireEvent.click(screen.getByRole('button', { name: 'Expand source openmaptiles' }));

    expect((await screen.findByText('Planet, z0-14, 2 vector layers')).textContent).toBe(
      'Planet, z0-14, 2 vector layers',
    );
  });

  it('writes an edited zoom limit into the source', () => {
    renderSection(SourcesSection, sourcesStyle);

    fireEvent.click(screen.getByRole('button', { name: 'Expand source openmaptiles' }));
    fireEvent.change(
      within(screen.getByText('maxzoom').parentElement as HTMLElement).getByRole('spinbutton'),
      { target: { value: '10' } },
    );

    expect(recorded.map((r) => [r.label, r.style.sources.openmaptiles])).toEqual([
      [
        'Set maxzoom of source openmaptiles',
        { maxzoom: 10, type: 'vector', url: 'https://tiles.example.com/planet' },
      ],
    ]);
  });

  it('adds a Martin source that the style does not have yet', () => {
    renderSection(SourcesSection, sourcesStyle, {
      martinSources: [
        {
          id: 'world_cities',
          name: 'world_cities',
          spec: { type: 'vector', url: 'http://m/world_cities' },
        },
        { id: 'other', name: 'other', spec: { type: 'vector', url: 'http://m/other' } },
      ],
    });

    fireEvent.click(screen.getByRole('button', { name: 'Add source' }));
    expect(screen.queryByRole('button', { name: 'Add other from Martin' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Add world_cities from Martin' }));

    expect(recorded.map((r) => [r.label, r.style.sources.world_cities])).toEqual([
      ['Add source world_cities', { type: 'vector', url: 'http://m/world_cities' }],
    ]);
  });

  it('adds a custom geojson source with its URL as data', () => {
    renderSection(SourcesSection, sourcesStyle);

    fireEvent.click(screen.getByRole('button', { name: 'Add source' }));
    fireEvent.change(screen.getByLabelText('Source type'), { target: { value: 'geojson' } });
    fireEvent.change(screen.getByLabelText('Source id'), { target: { value: 'places' } });
    fireEvent.change(screen.getByLabelText('Source URL'), {
      target: { value: 'https://example.com/p.geojson' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));

    expect(recorded[0].style.sources.places).toEqual({
      data: 'https://example.com/p.geojson',
      type: 'geojson',
    });
  });
});
