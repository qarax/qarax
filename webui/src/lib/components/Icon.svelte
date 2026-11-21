<script lang="ts" module>
	// PatternFly's own icons (MIT): the status set its products use for VMs and
	// hosts, from the package's icons/, and the alert and search glyphs its
	// components use, from its docs/ examples.
	import ok from '@patternfly/patternfly/icons/PfIcons/ok.svg?raw';
	import off from '@patternfly/patternfly/icons/PfIcons/off.svg?raw';
	import paused from '@patternfly/patternfly/icons/PfIcons/paused.svg?raw';
	import inProgress from '@patternfly/patternfly/icons/PfIcons/in-progress.svg?raw';
	import unknown from '@patternfly/patternfly/icons/PfIcons/unknown.svg?raw';
	import errorCircle from '@patternfly/patternfly/icons/PfIcons/error-circle-o.svg?raw';
	import server from '@patternfly/patternfly/icons/PfIcons/server.svg?raw';
	import virtualMachine from '@patternfly/patternfly/icons/PfIcons/virtual-machine.svg?raw';
	import disconnected from '@patternfly/patternfly/icons/PfIcons/disconnected.svg?raw';
	import storage from '@patternfly/patternfly/icons/PfIcons/storage-domain.svg?raw';
	import network from '@patternfly/patternfly/icons/PfIcons/network.svg?raw';

	const glyph = (viewBox: string, d: string) =>
		`<svg fill="currentColor" viewBox="${viewBox}"><path d="${d}" /></svg>`;

	const SOURCES = {
		ok,
		off,
		paused,
		inProgress,
		unknown,
		error: errorCircle,
		server,
		virtualMachine,
		disconnected,
		storage,
		network,
		danger: glyph(
			'0 0 32 32',
			'M16 1C7.729 1 1 7.729 1 16s6.729 15 15 15 15-6.729 15-15S24.271 1 16 1Zm-1.5 8a1.5 1.5 0 1 1 3 0v7a1.5 1.5 0 1 1-3 0V9ZM16 25.001a2 2 0 1 1-.001-3.999A2 2 0 0 1 16 25.001Z'
		),
		warning: glyph(
			'0 0 32 32',
			'm31.874 28.514-15.011-27a1.001 1.001 0 0 0-1.748 0l-15.011 27A1 1 0 0 0 .978 30H31a1 1 0 0 0 .874-1.486ZM14.5 12a1.5 1.5 0 0 1 3 0v5a1.5 1.5 0 0 1-3 0v-5ZM16 26.001a2 2 0 1 1-.001-3.999A2 2 0 0 1 16 26.001Z'
		),
		search: glyph(
			'0 0 32 32',
			'm30.796 29.205-8.557-8.557A11.945 11.945 0 0 0 25 13c0-6.617-5.383-12-12-12S1 6.383 1 13s5.383 12 12 12c2.904 0 5.57-1.038 7.648-2.761l8.556 8.556a1.122 1.122 0 0 0 1.592 0 1.127 1.127 0 0 0 0-1.591ZM3 13C3 7.486 7.486 3 13 3s10 4.486 10 10-4.486 10-10 10S3 18.514 3 13Z'
		),
		caretDown: glyph(
			'0 0 32 32',
			'M29.707 11.707 17.061 24.353c-.292.292-.677.438-1.061.438s-.769-.146-1.061-.438L2.293 11.707A1 1 0 0 1 3 10h26a1 1 0 0 1 .707 1.707Z'
		),
		close: glyph(
			'0 0 20 20',
			'M17.8 16.2 11.59 10l6.21-6.21c.42-.46.39-1.17-.07-1.59-.43-.4-1.09-.4-1.52 0l-6.2 6.2-6.22-6.19c-.44-.44-1.15-.44-1.59 0-.44.44-.44 1.15 0 1.59l6.2 6.21-6.2 6.2c-.42.46-.39 1.17.07 1.59.43.4 1.09.4 1.52 0L10 11.59l6.2 6.2c.44.44 1.15.44 1.59 0 .44-.45.44-1.16 0-1.6Z'
		),
		chevronRight: glyph(
			'0 0 20 20',
			'M14.35 8.94 6.71 1.29l-.02-.02c-.4-.38-1.03-.37-1.41.02-.38.4-.37 1.03.02 1.41l7.29 7.29-7.29 7.29a1.003 1.003 0 0 0 1.42 1.42l7.65-7.65c.59-.59.59-1.54 0-2.12Z'
		)
	};

	export type IconName = keyof typeof SOURCES;

	/** The <svg> alone (the files start with an XML declaration), sized to the text. */
	const MARKUP = Object.fromEntries(
		Object.entries(SOURCES).map(([name, source]) => [
			name,
			source
				.slice(source.indexOf('<svg'))
				.replace(' role="img"', '')
				.replace('<svg ', '<svg class="pf-v6-svg" width="1em" height="1em" aria-hidden="true" ')
		])
	) as Record<IconName, string>;
</script>

<script lang="ts">
	let { name }: { name: IconName } = $props();
</script>

<!-- {@html}: PatternFly's own SVG markup, never user input. -->
{@html MARKUP[name]}
