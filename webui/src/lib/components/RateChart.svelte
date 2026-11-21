<script lang="ts">
	import { ago, bytes } from '#lib/format.ts';
	import { WINDOW_MS, type RatePoint } from '#lib/metrics.svelte.ts';

	/**
	 * Rates in bytes per second over up to the last {@link WINDOW_MS}, one 2px line per
	 * series on one axis, with a legend. Hover or the arrow keys move a crosshair
	 * whose tooltip gives every series' value at that moment.
	 *
	 * Series take fixed colours in order (PatternFly's blue, then orange), checked
	 * with the dataviz palette validator against the card surface of each theme.
	 */
	let {
		label,
		series,
		points,
		end
	}: {
		label: string;
		series: readonly { label: string }[];
		points: (RatePoint | null)[];
		end: number;
	} = $props();

	/** The chart spans what has been read, from a minute up to the whole window. */
	const MIN_SPAN_MS = 60_000;

	const HEIGHT = 180;
	const TOP = 12;
	const BOTTOM = 24;
	const LEFT = 84;
	const RIGHT = 16;
	const plotHeight = HEIGHT - TOP - BOTTOM;

	let width = $state(0);
	const plotWidth = $derived(Math.max(0, width - LEFT - RIGHT));
	const first = $derived(points.find((point) => point !== null)?.at ?? end);
	const span = $derived(Math.min(WINDOW_MS, Math.max(MIN_SPAN_MS, end - first)));
	const start = $derived(end - span);
	const readings = $derived(
		points.filter((point): point is RatePoint => point !== null && point.at >= start)
	);
	const scale = $derived(niceScale(Math.max(0, ...readings.flatMap((point) => point.values))));

	const x = (at: number) => LEFT + ((at - start) / span) * plotWidth;
	const y = (value: number) => TOP + plotHeight * (1 - value / scale.top);
	const rate = (value: number) => `${bytes(value)}/s`;

	/**
	 * An axis from 0 to a top that holds `max`, in about four steps of a power
	 * of two in binary units (256 B, 1 KiB, 4 MiB), so the ticks read cleanly.
	 * An idle device still gets an axis up to 1 KiB/s.
	 */
	function niceScale(max: number): { top: number; ticks: number[] } {
		const reach = Math.max(max, 1024);
		const raw = reach / 4;
		const unit = 1024 ** Math.floor(Math.log(raw) / Math.log(1024));
		let step = unit;
		while (step < raw) step *= 2;
		const top = Math.ceil(reach / step) * step;
		return { top, ticks: Array.from({ length: top / step + 1 }, (_, i) => i * step) };
	}

	/** One series as SVG path data, lifting the pen where the readings break. */
	function path(index: number): string {
		let d = '';
		let pen = false;
		for (const point of points) {
			if (point === null || point.at < start) {
				pen = false;
				continue;
			}
			d += `${pen ? 'L' : 'M'}${x(point.at).toFixed(1)},${y(point.values[index]).toFixed(1)}`;
			pen = true;
		}
		return d;
	}

	// The reading under the crosshair, by its time, so it stays put as new ones arrive.
	let activeAt: number | null = $state(null);
	const active = $derived(readings.find((point) => point.at === activeAt) ?? null);
	const latest = $derived(readings.at(-1) ?? null);

	function point(event: PointerEvent) {
		const left = (event.currentTarget as Element).getBoundingClientRect().left;
		const px = event.clientX - left;
		let best: RatePoint | null = null;
		for (const reading of readings)
			if (best === null || Math.abs(x(reading.at) - px) < Math.abs(x(best.at) - px)) best = reading;
		activeAt = best?.at ?? null;
	}

	function key(event: KeyboardEvent) {
		if (!readings.length) return;
		const at = readings.findIndex((reading) => reading.at === activeAt);
		const current = at < 0 ? readings.length - 1 : at;
		const next: Record<string, number> = {
			ArrowLeft: Math.max(0, current - 1),
			ArrowRight: Math.min(readings.length - 1, current + 1),
			Home: 0,
			End: readings.length - 1
		};
		if (event.key === 'Escape') {
			activeAt = null;
			return;
		}
		if (!(event.key in next)) return;
		event.preventDefault();
		activeAt = readings[next[event.key]].at;
	}

	// For assistive technology the chart is a slider over its readings: the
	// arrow keys move it, and each step reads out that moment's values.
	const shown = $derived(active ?? latest);
	const valueText = $derived(
		shown
			? `${ago(new Date(shown.at).toISOString())}: ${series
					.map((s, i) => `${s.label.toLowerCase()} ${rate(shown.values[i])}`)
					.join(', ')}`
			: 'No readings yet'
	);
</script>

<ul class="legend" role="list">
	{#each series as s, i (s.label)}
		<li>
			<svg width="16" height="8" aria-hidden="true">
				<line class="series-{i}" x1="0" y1="4" x2="16" y2="4" />
			</svg>
			{s.label}
		</li>
	{/each}
</ul>

<div
	class="chart"
	bind:clientWidth={width}
	role="slider"
	tabindex="0"
	aria-label="{label}, the last {Math.round(span / 60_000)} min"
	aria-valuemin={0}
	aria-valuemax={Math.max(0, readings.length - 1)}
	aria-valuenow={shown ? readings.indexOf(shown) : 0}
	aria-valuetext={valueText}
	onkeydown={key}
	onfocus={() => (activeAt = latest?.at ?? null)}
	onblur={() => (activeAt = null)}
>
	{#if width > 0}
		<svg
			{width}
			height={HEIGHT}
			aria-hidden="true"
			onpointermove={point}
			onpointerleave={() => (activeAt = null)}
		>
			{#each scale.ticks as tick (tick)}
				<line class="grid" x1={LEFT} x2={LEFT + plotWidth} y1={y(tick)} y2={y(tick)} />
				<text class="axis" x={LEFT - 8} y={y(tick)} text-anchor="end" dominant-baseline="middle"
					>{rate(tick)}</text
				>
			{/each}
			<text class="axis" x={LEFT} y={HEIGHT - 6} text-anchor="start"
				>{Math.round(span / 60_000)} min ago</text
			>
			<text class="axis" x={LEFT + plotWidth} y={HEIGHT - 6} text-anchor="end">now</text>
			{#each series as s, i (s.label)}
				<path class="line series-{i}" d={path(i)} />
			{/each}
			{#if active}
				<line
					class="crosshair"
					x1={x(active.at)}
					x2={x(active.at)}
					y1={TOP}
					y2={TOP + plotHeight}
				/>
			{/if}
			{#if latest || active}
				{@const marked = active ?? latest}
				{#if marked}
					{#each series as s, i (s.label)}
						<circle class="dot series-{i}" cx={x(marked.at)} cy={y(marked.values[i])} r="4" />
					{/each}
				{/if}
			{/if}
		</svg>
		{#if active}
			<div
				class="pf-v6-c-tooltip pf-m-top tip"
				role="tooltip"
				style:left="{Math.min(Math.max(x(active.at), 90), width - 90)}px"
				style:top="{TOP}px"
			>
				<div class="pf-v6-c-tooltip__arrow"></div>
				<div class="pf-v6-c-tooltip__content">
					<div class="when">{ago(new Date(active.at).toISOString())}</div>
					{#each series as s, i (s.label)}
						<div class="reading">
							<svg width="12" height="8" aria-hidden="true">
								<line class="series-{i} on-tooltip" x1="0" y1="4" x2="12" y2="4" />
							</svg>
							<strong>{rate(active.values[i])}</strong>
							{s.label.toLowerCase()}
						</div>
					{/each}
				</div>
			</div>
		{/if}
	{/if}
</div>

<style>
	.legend {
		display: flex;
		flex-wrap: wrap;
		gap: var(--pf-t--global--spacer--md);
		margin: 0 0 var(--pf-t--global--spacer--sm);
		padding: 0;
		list-style: none;
		font-size: var(--pf-t--global--font--size--body--sm);
	}

	.legend li {
		display: inline-flex;
		align-items: center;
		gap: var(--pf-t--global--spacer--xs);
	}

	.chart {
		position: relative;
		border-radius: var(--pf-t--global--border--radius--small);
	}

	.chart:focus-visible {
		outline: var(--pf-t--global--border--width--strong) solid
			var(--pf-t--global--border--color--clicked);
		outline-offset: 2px;
	}

	svg {
		display: block;
		overflow: visible;
	}

	.grid {
		stroke: var(--pf-t--global--border--color--default);
		stroke-width: 1;
		shape-rendering: crispEdges;
	}

	.axis {
		fill: var(--pf-t--global--text--color--subtle);
		font-size: var(--pf-t--global--font--size--body--sm);
		font-variant-numeric: tabular-nums;
	}

	.line {
		fill: none;
		stroke-width: 2;
		stroke-linejoin: round;
		stroke-linecap: round;
	}

	/* The legend's and tooltip's line keys match the lines. */
	line[class^='series-'] {
		stroke-width: 2;
		stroke-linecap: round;
	}

	.dot {
		stroke: var(--pf-t--global--background--color--primary--default);
		stroke-width: 2;
	}

	.crosshair {
		stroke: var(--pf-t--global--text--color--subtle);
		stroke-width: 1;
		shape-rendering: crispEdges;
	}

	/* Blue then orange: blue-50 on a light surface, blue-40 on a dark one.
	   PatternFly's tooltip is dark in the light theme and light in the dark one. */
	line.series-0,
	.line.series-0 {
		stroke: var(--pf-t--color--blue--50);
	}
	.dot.series-0 {
		fill: var(--pf-t--color--blue--50);
	}
	line.series-1,
	.line.series-1 {
		stroke: var(--pf-t--color--orange--50);
	}
	.dot.series-1 {
		fill: var(--pf-t--color--orange--50);
	}
	:global(.pf-v6-theme-dark) line.series-0,
	:global(.pf-v6-theme-dark) .line.series-0,
	line.series-0.on-tooltip {
		stroke: var(--pf-t--color--blue--40);
	}
	:global(.pf-v6-theme-dark) .dot.series-0 {
		fill: var(--pf-t--color--blue--40);
	}
	:global(.pf-v6-theme-dark) line.series-0.on-tooltip {
		stroke: var(--pf-t--color--blue--50);
	}

	.tip {
		position: absolute;
		transform: translate(-50%, calc(-100% - var(--pf-t--global--spacer--sm)));
		pointer-events: none;
		white-space: nowrap;
	}

	.when {
		margin-bottom: var(--pf-t--global--spacer--xs);
	}

	.reading {
		display: flex;
		align-items: center;
		gap: var(--pf-t--global--spacer--xs);
		font-variant-numeric: tabular-nums;
	}
</style>
