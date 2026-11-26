import { useEffect, useRef, useState, type ReactNode } from 'react';
import { cn } from '../lib/utils';

export interface OutputStream {
  id: string;
  label: string;
  text: string;
}

/** The dark log panel: stream tabs on top, the selected stream below, following new output while scrolled to the end. */
export function OutputPanel({ streams, empty = '暂无输出', aside, className, label = '执行输出' }: {
  streams: OutputStream[];
  empty?: string;
  aside?: ReactNode;
  className?: string;
  label?: string;
}) {
  const available = streams.filter(stream => stream.text);
  const [selected, setSelected] = useState<string | null>(null);
  const current = available.find(stream => stream.id === selected) ?? available[0];
  const scroller = useRef<HTMLPreElement>(null);
  const following = useRef(true);

  useEffect(() => {
    const node = scroller.current;
    if (node && following.current) node.scrollTop = node.scrollHeight;
  }, [current?.text]);

  return <div className={cn('dash-output-panel', className)} data-output-panel>
    <div className="dash-log-tabs" role="tablist" aria-label={label}>
      {available.length > 0 ? available.map(stream => <button key={stream.id} type="button" role="tab" className="dash-log-tab"
        aria-selected={stream.id === current?.id} onClick={() => { setSelected(stream.id); following.current = true; }}>
        {stream.label}
      </button>) : <span className="dash-log-tab" aria-hidden="true">输出</span>}
      {aside && <span className="dash-log-aside">{aside}</span>}
    </div>
    {current ? <pre ref={scroller} className="dash-code" tabIndex={0} aria-label={label + ' · ' + current.label} data-output-stream={current.id}
      onScroll={event => {
        const node = event.currentTarget;
        following.current = node.scrollHeight - node.scrollTop - node.clientHeight < 24;
      }}>{current.text}</pre>
      : <p className="dash-log-empty">{empty}</p>}
  </div>;
}

export function executionStreams(run: { stdout?: string; stderr?: string } | null | undefined): OutputStream[] {
  return [
    { id: 'stdout', label: '标准输出', text: run?.stdout ?? '' },
    { id: 'stderr', label: '错误输出', text: run?.stderr ?? '' },
  ];
}
