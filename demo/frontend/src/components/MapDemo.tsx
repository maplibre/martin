import { Layers } from 'lucide-react';
import { useCallback, useEffect, useId, useState } from 'react';
import BottomSheet from '@/components/BottomSheet';
import type { FilterState } from '@/components/demo/FilterPanel';
import FilterPanel, { getInitialFilterState } from '@/components/demo/FilterPanel';
import MetricsPanel from '@/components/demo/MetricsPanel';
import type { DemoScenarioItem } from '@/components/demo/ScenarioSelector';
import ScenarioSelector from '@/components/demo/ScenarioSelector';
import StylingPanel from '@/components/demo/StylingPanel';
import TimeSlider from '@/components/demo/TimeSlider';
import type { HoveredFeature, MapStylingOptions } from '@/components/martin-map';
import MartinMap from '@/components/martin-map';
import { getSqlDisplay } from '@/lib/demo-utils';
import { useMediaQueryMinHeight, useMediaQuerySm } from '@/lib/useMediaQuery';
import type { DemoLayerEntry } from '@/types/demo';

interface MapDemoProps {
  tileSources: DemoLayerEntry[];
  demoScenarios: DemoScenarioItem[];
  martinBaseUrl: string;
}

export default function MapDemo({ tileSources, demoScenarios, martinBaseUrl }: MapDemoProps) {
  const [activeLayer, setActiveLayer] = useState(tileSources[0]?.id ?? '');
  const activeLayerConfig = tileSources.find((s) => s.id === activeLayer) ?? null;
  const [filterState, setFilterState] = useState<FilterState>(() =>
    getInitialFilterState(activeLayerConfig),
  );
  const [hovered, setHovered] = useState<HoveredFeature | null>(null);
  const [showSql, setShowSql] = useState(false);
  const [styling, setStyling] = useState<MapStylingOptions>({
    fontStack: 'default',
    spriteType: 'sdf',
    styleId: 'dark',
  });
  const [mobileSheetOpen, setMobileSheetOpen] = useState(false);
  const isWide = useMediaQuerySm();
  const isTall = useMediaQueryMinHeight();
  const isDesktop = isWide && isTall;
  const sqlModalTitleId = useId();

  useEffect(() => {
    setFilterState(getInitialFilterState(activeLayerConfig));
  }, [activeLayerConfig]);

  const handleFilterChange = useCallback((name: string, value: string | number) => {
    setFilterState((prev) => ({ ...prev, [name]: value }));
  }, []);

  const handleScenarioSelect = useCallback((preset: Record<string, string | number>) => {
    setFilterState((prev) => ({ ...prev, ...preset }));
  }, []);

  const sqlText =
    activeLayerConfig != null
      ? getSqlDisplay(activeLayerConfig.sqlTemplate, hovered?.name ?? null, filterState)
      : '';

  return (
    <div className="absolute inset-0">
      <MartinMap
        activeLayer={activeLayer}
        filterState={filterState}
        martinBaseUrl={martinBaseUrl}
        onActiveLayerChange={setActiveLayer}
        onHoveredChange={setHovered}
        styling={styling}
        tileSources={tileSources}
      />

      {isDesktop && (
        <div className="absolute bottom-2 left-1/2 z-10 grid grid-cols-[auto_minmax(0,520px)_auto] items-end gap-3 w-[min(900px,calc(100%-2rem))] -translate-x-1/2 pointer-events-auto">
          <div className="bg-background/95 backdrop-blur-md border border-border rounded-xl shadow-xl overflow-hidden min-w-0">
            <div className="flex items-center gap-2 px-3 py-2 border-b border-border">
              <span className="text-[10px] font-mono text-muted-foreground uppercase tracking-wider">
                Styling
              </span>
            </div>
            <div className="px-3 py-3">
              <StylingPanel
                fontStack={styling.fontStack}
                onFontChange={(fontStack) => setStyling((s) => ({ ...s, fontStack }))}
                onSpriteTypeChange={(spriteType) => setStyling((s) => ({ ...s, spriteType }))}
                onStyleChange={(styleId) => setStyling((s) => ({ ...s, styleId }))}
                spriteType={styling.spriteType}
                styleId={styling.styleId}
              />
            </div>
          </div>

          <div className="bg-background/95 backdrop-blur-md border border-border rounded-xl shadow-xl overflow-hidden min-w-0">
            <div className="flex flex-wrap items-center gap-2 px-3 py-2 border-b border-border">
              <div className="flex flex-wrap gap-1">
                {tileSources.map((s) => (
                  <button
                    className={`px-2 py-1 min-h-[24px] min-w-[24px] text-[10px] font-mono rounded transition-colors ${
                      activeLayer === s.id
                        ? 'bg-primary text-primary-foreground'
                        : 'text-muted-foreground hover:text-foreground'
                    }`}
                    key={s.id}
                    onClick={() => setActiveLayer(s.id)}
                    type="button"
                  >
                    {s.label}
                  </button>
                ))}
              </div>
              <span className="text-[10px] font-mono text-border select-none px-1">/</span>
              <span className="text-[10px] font-mono text-muted-foreground">Filters</span>
              {hovered != null && (
                <span className="ml-auto text-[10px] font-mono text-accent truncate max-w-[140px] shrink-0">
                  ↗ {hovered.name}
                  {hovered.trips != null
                    ? ` · ${hovered.trips} trips${hovered.trips_price != null ? ` · $${hovered.trips_price} avg` : ''}`
                    : hovered.pop_est != null
                      ? ` · ${(hovered.pop_est / 1_000_000).toFixed(1)}M`
                      : ''}
                </span>
              )}
            </div>
            <div className="px-3 py-3 flex flex-col gap-3 max-h-[min(40vh,360px)] overflow-y-auto">
              <div className="flex flex-wrap items-start gap-4 gap-y-3">
                <FilterPanel
                  filterState={filterState}
                  layer={activeLayerConfig}
                  onFilterChange={handleFilterChange}
                />
                <TimeSlider
                  filterState={filterState}
                  layer={activeLayerConfig}
                  onFilterChange={handleFilterChange}
                />
                <ScenarioSelector
                  activeLayerId={activeLayer}
                  filterState={filterState}
                  onSelectScenario={handleScenarioSelect}
                  scenarios={demoScenarios}
                />
              </div>
              <div className="border-t border-border pt-2">
                <button
                  className="text-[10px] font-mono text-muted-foreground hover:text-foreground transition-colors"
                  onClick={() => setShowSql(true)}
                  type="button"
                >
                  Show executed SQL
                </button>
              </div>
            </div>
          </div>

          <div className="bg-background/95 backdrop-blur-md border border-border rounded-xl shadow-xl overflow-hidden min-w-0">
            <div className="flex items-center gap-2 px-3 py-2 border-b border-border">
              <span className="text-[10px] font-mono text-muted-foreground uppercase tracking-wider">
                Observability
              </span>
            </div>
            <div className="px-3 py-3">
              <MetricsPanel hideTitle martinBaseUrl={martinBaseUrl} />
            </div>
          </div>
        </div>
      )}

      {!isDesktop && (
        <>
          <button
            aria-label="Open demo controls"
            className="absolute bottom-4 left-1/2 z-10 -translate-x-1/2 pointer-events-auto flex items-center gap-2 px-4 py-3 text-sm font-mono bg-background/95 backdrop-blur-md border border-border rounded-xl shadow-xl text-foreground hover:border-primary/50 transition-colors"
            onClick={() => setMobileSheetOpen(true)}
            type="button"
          >
            <Layers className="size-4 shrink-0" />
            Layers & filters
          </button>
          <BottomSheet
            onClose={() => setMobileSheetOpen(false)}
            open={mobileSheetOpen}
            title="Demo controls"
          >
            <div className="p-4 flex flex-col gap-6">
              <section>
                <h3 className="text-[10px] font-mono text-muted-foreground uppercase tracking-wider mb-3">
                  Styling
                </h3>
                <StylingPanel
                  fontStack={styling.fontStack}
                  onFontChange={(fontStack) => setStyling((s) => ({ ...s, fontStack }))}
                  onSpriteTypeChange={(spriteType) => setStyling((s) => ({ ...s, spriteType }))}
                  onStyleChange={(styleId) => setStyling((s) => ({ ...s, styleId }))}
                  spriteType={styling.spriteType}
                  styleId={styling.styleId}
                />
              </section>
              <section>
                <h3 className="text-[10px] font-mono text-muted-foreground uppercase tracking-wider mb-2">
                  Layers
                </h3>
                <div className="flex flex-wrap gap-1 mb-3">
                  {tileSources.map((s) => (
                    <button
                      className={`px-2 py-1 min-h-[24px] min-w-[24px] text-[10px] font-mono rounded transition-colors ${
                        activeLayer === s.id
                          ? 'bg-primary text-primary-foreground'
                          : 'text-muted-foreground hover:text-foreground border border-border'
                      }`}
                      key={s.id}
                      onClick={() => setActiveLayer(s.id)}
                      type="button"
                    >
                      {s.label}
                    </button>
                  ))}
                </div>
                <h3 className="text-[10px] font-mono text-muted-foreground uppercase tracking-wider mb-2">
                  Filters
                </h3>
                <div className="flex flex-wrap items-start gap-4 gap-y-3">
                  <FilterPanel
                    filterState={filterState}
                    layer={activeLayerConfig}
                    onFilterChange={handleFilterChange}
                  />
                  <TimeSlider
                    filterState={filterState}
                    layer={activeLayerConfig}
                    onFilterChange={handleFilterChange}
                  />
                  <ScenarioSelector
                    activeLayerId={activeLayer}
                    filterState={filterState}
                    onSelectScenario={handleScenarioSelect}
                    scenarios={demoScenarios}
                  />
                </div>
                <div className="border-t border-border pt-2 mt-3">
                  <button
                    className="text-[10px] font-mono text-muted-foreground hover:text-foreground transition-colors"
                    onClick={() => setShowSql(true)}
                    type="button"
                  >
                    Show executed SQL
                  </button>
                </div>
              </section>
              <section>
                <h3 className="text-[10px] font-mono text-muted-foreground uppercase tracking-wider mb-3">
                  Observability
                </h3>
                <MetricsPanel hideTitle martinBaseUrl={martinBaseUrl} />
              </section>
            </div>
          </BottomSheet>
        </>
      )}

      {showSql && (
        <div
          aria-labelledby={sqlModalTitleId}
          aria-modal="true"
          className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/50 backdrop-blur-sm"
          onClick={() => setShowSql(false)}
          onKeyDown={(e) => e.key === 'Escape' && setShowSql(false)}
          role="dialog"
        >
          <div
            className="bg-background border border-border rounded-xl shadow-xl max-w-2xl w-full max-h-[80vh] overflow-hidden flex flex-col"
            onClick={(e) => e.stopPropagation()}
            onKeyDown={(e) => e.stopPropagation()}
            role="document"
          >
            <div className="flex items-center justify-between px-4 py-2 border-b border-border shrink-0">
              <span className="text-sm font-mono font-medium" id={sqlModalTitleId}>
                Executed SQL
              </span>
              <button
                aria-label="Close"
                className="text-muted-foreground hover:text-foreground transition-colors p-1 rounded"
                onClick={() => setShowSql(false)}
                type="button"
              >
                <span className="text-sm font-mono">Close</span>
              </button>
            </div>
            <pre className="p-4 overflow-auto text-[11px] font-mono text-foreground/80 leading-relaxed whitespace-pre shrink min-h-0">
              <code>
                {sqlText.split('\n').map((line, i) => {
                  const isComment = line.trimStart().startsWith('--');
                  return (
                    // biome-ignore lint/suspicious/noArrayIndexKey: SQL lines have no stable identity
                    <span className={isComment ? 'text-accent' : undefined} key={i}>
                      {line}
                      {'\n'}
                    </span>
                  );
                })}
              </code>
            </pre>
          </div>
        </div>
      )}
    </div>
  );
}
