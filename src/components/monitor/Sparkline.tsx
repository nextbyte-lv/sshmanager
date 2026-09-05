interface SparklineProps {
    values: number[];
    /**
     * Fixed top of the scale — 100 for a percentage, so the line means the same
     * thing from one refresh to the next. Left out, the scale follows the data,
     * which is what a throughput graph wants.
     */
    max?: number;
    className?: string;
}

// Hand-rolled rather than pulling in a charting library: it is one polyline, and
// a dependency that ships its own renderer would dwarf the feature using it.
//
// The viewBox is a fixed grid with `preserveAspectRatio="none"`, so the SVG
// stretches to whatever box the card gives it and the stroke stays hairline.
const VIEW_WIDTH = 100;
const VIEW_HEIGHT = 30;

export function Sparkline({ values, max, className }: SparklineProps) {
    if (values.length < 2) {
        return <div className={className} />;
    }

    const ceiling = Math.max(max ?? Math.max(...values), 1);
    const step = VIEW_WIDTH / (values.length - 1);
    const points = values.map((value, index) => {
        const clamped = Math.min(Math.max(value, 0), ceiling);
        const y = VIEW_HEIGHT - (clamped / ceiling) * VIEW_HEIGHT;
        return `${(index * step).toFixed(2)},${y.toFixed(2)}`;
    });

    return (
        <svg
            className={className}
            viewBox={`0 0 ${VIEW_WIDTH} ${VIEW_HEIGHT}`}
            preserveAspectRatio="none"
            aria-hidden="true"
        >
            {/* Closed back along the baseline so the area under the line can be
                tinted; the stroke is drawn separately so the baseline itself
                doesn't show. */}
            <polygon
                points={`0,${VIEW_HEIGHT} ${points.join(" ")} ${VIEW_WIDTH},${VIEW_HEIGHT}`}
                fill="currentColor"
                opacity={0.15}
            />
            <polyline
                points={points.join(" ")}
                fill="none"
                stroke="currentColor"
                strokeWidth={1}
                vectorEffect="non-scaling-stroke"
            />
        </svg>
    );
}
