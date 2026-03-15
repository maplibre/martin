import { BookOpen, Database, ExternalLink, Github, Layers, Shield, Users, Zap } from 'lucide-react';
import { useCallback, useEffect, useId, useState } from 'react';
import BottomSheet from '@/components/BottomSheet';
import FilterPanel, {
  type FilterState,
  getInitialFilterState,
} from '@/components/demo/FilterPanel';
import MetricsPanel from '@/components/demo/MetricsPanel';
import type { DemoScenarioItem } from '@/components/demo/ScenarioSelector';
import ScenarioSelector from '@/components/demo/ScenarioSelector';
import StylingPanel from '@/components/demo/StylingPanel';
import TimeSlider from '@/components/demo/TimeSlider';
import HeroTitleBlock from '@/components/HeroTitleBlock';
import InstallBox from '@/components/install-box';
import MartinMap, { type MapStylingOptions } from '@/components/martin-map';
import NavTooltip from '@/components/nav-tooltip';
import { getSqlDisplay } from '@/lib/demo-utils';
import { useMediaQueryMinHeight, useMediaQuerySm } from '@/lib/useMediaQuery';
import type { DemoLayerEntry, HoveredFeature } from '@/types/demo';

const ICON_MAP = {
  Database,
  Layers,
  Shield,
  Users,
  Zap,
} as const;

export interface GithubStats {
  stars: string;
  contributors: string;
  forks: string;
  latestVersion: string;
}

export interface FeatureItem {
  title: string;
  body: string;
  icon: keyof typeof ICON_MAP;
}

interface PageContentProps {
  stats: GithubStats;
  features?: FeatureItem[];
  tileSources?: DemoLayerEntry[];
  demoScenarios?: DemoScenarioItem[];
  martinBaseUrl: string;
}

export default function PageContent({
  stats,
  features = [],
  tileSources = [],
  demoScenarios = [],
  martinBaseUrl,
}: PageContentProps) {
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
  const isTallViewport = useMediaQueryMinHeight();
  /** Single hero (map behind title) only when wide and tall; otherwise title then map. */
  const useSingleHero = isWide && isTallViewport;
  const sqlModalTitleId = useId();
  const featuresHeadingId = useId();
  const installHeadingId = useId();
  const communityHeadingId = useId();

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
  const STATS = [
    { label: 'GitHub stars', value: stats.stars },
    { label: 'Contributors', value: stats.contributors },
    { label: 'Forks', value: stats.forks },
    { label: 'Latest release', value: stats.latestVersion?.replace('martin-', '') ?? 'v0.x' },
    { label: 'License', value: 'Apache 2.0 + MIT' },
    { label: 'Tile formats', value: 'MVT + MLT + Raster' },
  ];

  return (
    <div className="min-h-screen bg-background text-foreground flex flex-col font-sans">
      <header className="border-b border-border px-6 py-4">
        <div className="max-w-4xl mx-auto flex items-center justify-between">
          <div className="flex items-center gap-3">
            <img alt="Martin tile server" className="h-7 w-auto" src="/logo.svg" />
            <NavTooltip
              description="Martin is part of the MapLibre open-source ecosystem — community governed, vendor neutral."
              label="MapLibre"
            >
              <a
                className="text-xs font-mono text-muted-foreground border border-border rounded px-1.5 py-0.5 hover:text-foreground hover:border-primary/50 transition-colors"
                href="https://maplibre.org/"
                rel="noopener noreferrer"
                target="_blank"
              >
                by MapLibre
              </a>
            </NavTooltip>
          </div>
          <nav aria-label="Main navigation" className="flex items-center gap-5">
            <NavTooltip
              description="Access comprehensive guides and documentation for the Martin tile server."
              label="Martin Documentation"
            >
              <a
                className="text-sm text-muted-foreground hover:text-foreground transition-colors"
                href="https://maplibre.org/martin/introduction.html"
                rel="noopener noreferrer"
                target="_blank"
              >
                Docs
              </a>
            </NavTooltip>
            <NavTooltip
              description="Browse release notes and download pre-built Martin binaries for your platform."
              label="Releases"
            >
              <a
                className="text-sm text-muted-foreground hover:text-foreground transition-colors hidden sm:block"
                href="https://github.com/maplibre/martin/releases"
                rel="noopener noreferrer"
                target="_blank"
              >
                Releases
              </a>
            </NavTooltip>
            <NavTooltip
              description="View the source code, open issues, submit pull requests, and follow development."
              label="Martin on GitHub"
            >
              <a
                className="flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground transition-colors"
                href="https://github.com/maplibre/martin"
                rel="noopener noreferrer"
                target="_blank"
              >
                <Github className="size-4" />
                <span className="hidden sm:inline">GitHub</span>
              </a>
            </NavTooltip>
          </nav>
        </div>
      </header>

      <main className="flex flex-col flex-1">
        {useSingleHero ? (
          <section
            className="relative border-b border-border overflow-hidden"
            style={{ height: 'clamp(560px, 85vh, 880px)' }}
          >
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
            </div>
            <div className="absolute inset-0 bg-gradient-to-b from-background/95 via-background/60 to-transparent pointer-events-none" />
            <HeroTitleBlock />
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
                      {hovered.properties.trips != null
                        ? ` · ${hovered.properties.trips} trips${hovered.properties.trips_price != null ? ` · $${hovered.properties.trips_price} avg` : ''}`
                        : hovered.properties.pop_est != null
                          ? ` · ${((hovered.properties.pop_est as number) / 1_000_000).toFixed(1)}M`
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
          </section>
        ) : (
          <>
            <section className="border-b border-border">
              <HeroTitleBlock standalone />
            </section>
            <section
              className="relative border-b border-border overflow-hidden"
              style={{ height: 'clamp(400px, 60vh, 600px)' }}
            >
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
              </div>
              {isWide ? (
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
                        onSpriteTypeChange={(spriteType) =>
                          setStyling((s) => ({ ...s, spriteType }))
                        }
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
                          {hovered.properties.trips != null
                            ? ` · ${hovered.properties.trips} trips${hovered.properties.trips_price != null ? ` · $${hovered.properties.trips_price} avg` : ''}`
                            : hovered.properties.pop_est != null
                              ? ` · ${((hovered.properties.pop_est as number) / 1_000_000).toFixed(1)}M`
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
              ) : (
                <>
                  <button
                    aria-label="Open demo controls"
                    className="absolute bottom-14 left-1/2 z-10 -translate-x-1/2 pointer-events-auto flex items-center gap-2 px-4 py-3 text-sm font-mono bg-background/95 backdrop-blur-md border border-border rounded-xl shadow-xl text-foreground hover:border-primary/50 transition-colors"
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
                          onSpriteTypeChange={(spriteType) =>
                            setStyling((s) => ({ ...s, spriteType }))
                          }
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
            </section>
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

        <section aria-label="Project statistics" className="border-b border-border px-6 py-6">
          <div className="grid grid-cols-3 gap-x-6 gap-y-8 max-w-4xl mx-auto">
            {STATS.map(({ value, label }) => (
              <div className="flex flex-col gap-1" key={label}>
                <span className="text-2xl font-mono font-bold text-accent">{value}</span>
                <span className="text-xs text-muted-foreground">{label}</span>
              </div>
            ))}
          </div>
        </section>

        <section aria-labelledby={featuresHeadingId} className="px-6 py-12 border-b border-border">
          <div className="max-w-4xl mx-auto">
            <h2
              className="text-xs font-mono text-muted-foreground uppercase tracking-widest mb-6"
              id={featuresHeadingId}
            >
              Why Martin
            </h2>
            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
              {features.map(({ icon: iconKey, title, body }) => {
                const Icon = ICON_MAP[iconKey];
                return (
                  <div
                    className="bg-card border border-border rounded-lg p-5 flex flex-col gap-3"
                    key={title}
                  >
                    <div className="flex items-center gap-2.5">
                      <div className="p-1.5 rounded bg-primary/15 text-accent">
                        {Icon ? <Icon className="size-4 shrink-0" /> : null}
                      </div>
                      <span className="text-sm font-mono font-semibold text-foreground leading-snug">
                        {title}
                      </span>
                    </div>
                    <p className="text-xs leading-relaxed text-muted-foreground">{body}</p>
                  </div>
                );
              })}
            </div>
          </div>
        </section>

        <section aria-labelledby={installHeadingId} className="px-6 py-12 border-b border-border">
          <div className="max-w-4xl mx-auto grid grid-cols-1 lg:grid-cols-2 gap-8 items-start">
            <div>
              <h2
                className="text-xs font-mono text-muted-foreground uppercase tracking-widest mb-3"
                id={installHeadingId}
              >
                Quick start
              </h2>
              <h3 className="text-xl font-mono font-bold text-foreground mb-3">Up in seconds</h3>
              <p className="text-sm leading-relaxed text-muted-foreground mb-4">
                Install with Cargo, Homebrew, Docker, or grab a pre-built binary. Point Martin at
                your data source and you have a tile server.
              </p>
              <a
                className="inline-flex items-center gap-2 text-sm font-mono text-accent hover:text-foreground transition-colors"
                href="https://maplibre.org/martin/installation"
                rel="noopener noreferrer"
                target="_blank"
              >
                <BookOpen className="size-3.5" />
                Full installation guide
              </a>
            </div>
            <InstallBox />
          </div>
        </section>

        <section aria-labelledby={communityHeadingId} className="px-6 py-14">
          <div className="max-w-4xl mx-auto flex flex-col sm:flex-row items-start sm:items-center justify-between gap-6">
            <div>
              <h2
                className="text-xl font-mono font-bold text-foreground mb-2"
                id={communityHeadingId}
              >
                Get involved
              </h2>
              <p className="text-sm leading-relaxed text-muted-foreground max-w-sm text-pretty">
                Martin is built by the community. File issues, submit PRs, or join the discussion on
                GitHub and the MapLibre Slack.
              </p>
            </div>
            <div className="flex flex-wrap gap-3 shrink-0">
              <a
                className="inline-flex items-center gap-2 px-5 py-2.5 text-sm font-mono bg-primary text-primary-foreground rounded hover:opacity-90 transition-opacity"
                href="https://github.com/maplibre/martin/issues"
                rel="noopener noreferrer"
                target="_blank"
              >
                Open an issue
                <ExternalLink className="size-3.5" />
              </a>
              <a
                className="inline-flex items-center gap-2 px-5 py-2.5 text-sm font-mono bg-secondary text-secondary-foreground border border-border rounded hover:border-primary/50 transition-colors"
                href="https://maplibre.org/martin/development/"
                rel="noopener noreferrer"
                target="_blank"
              >
                Contributing guide
              </a>
            </div>
          </div>
        </section>
      </main>

      <footer className="border-t border-border px-6 py-5">
        <div className="max-w-4xl mx-auto flex flex-wrap items-center justify-between gap-3">
          <div className="flex items-center gap-3">
            <img alt="Martin tile server" className="h-5 w-auto" src="/logo.svg" />
            <span className="text-xs text-muted-foreground">© MapLibre contributors</span>
          </div>
          <nav
            aria-label="Footer navigation"
            className="flex items-center gap-5 text-xs font-mono text-muted-foreground"
          >
            <a
              className="hover:text-foreground transition-colors"
              href="https://maplibre.org/martin/introduction.html"
              rel="noopener noreferrer"
              target="_blank"
            >
              Docs
            </a>
            <a
              className="hover:text-foreground transition-colors"
              href="https://github.com/maplibre/martin/releases"
              rel="noopener noreferrer"
              target="_blank"
            >
              Releases
            </a>
            <a
              className="hover:text-foreground transition-colors"
              href="https://maplibre.org/"
              rel="noopener noreferrer"
              target="_blank"
            >
              MapLibre
            </a>
          </nav>
        </div>
      </footer>
    </div>
  );
}
