import { ComponentProps } from "react";
import { TimeseriesChart } from "@cloudflare/kumo";
import * as echarts from "echarts/core";
import { LineChart } from "echarts/charts";
import { AriaComponent, AxisPointerComponent, BrushComponent, GridComponent, MarkLineComponent, ToolboxComponent, TooltipComponent } from "echarts/components";
import { CanvasRenderer } from "echarts/renderers";

// Only the pieces Kumo's TimeseriesChart needs, so the rest of ECharts is tree-shaken out.
// This module is lazy-loaded (see HostCard) to keep ECharts out of the main bundle.
echarts.use([LineChart, AriaComponent, AxisPointerComponent, BrushComponent, GridComponent, MarkLineComponent, ToolboxComponent, TooltipComponent, CanvasRenderer]);

type Props = Pick<ComponentProps<typeof TimeseriesChart>, "data" | "height" | "isDarkMode" | "loading" | "ariaDescription">;

/** Percentage-over-time chart for host CPU and memory. */
export default function UsageChart(props: Props) {
  return (
    <TimeseriesChart
      {...props}
      echarts={echarts}
      gradient
      yAxisTickCount={3}
      // The axis auto-scales past 100 when usage is high; a percentage above that is meaningless.
      yAxisTickFormat={(v) => (v > 100 ? "" : `${v}%`)}
      xAxisTickCount={4}
      xAxisTickFormat={(ts) => new Date(ts).toLocaleTimeString([], { hourCycle: "h23", hour: "2-digit", minute: "2-digit", second: "2-digit" })}
      tooltipValueFormat={(v) => `${v.toFixed(1)}%`}
      tooltipFollowCursor="x"
    />
  );
}
