'use client';

import {useState, useEffect, useRef, SVGProps, FC} from 'react';
import cn from 'classnames';

interface PathProps extends SVGProps<SVGPathElement> {
  d: string;
  className?: string;
}

const Path: FC<PathProps> = ({d, className, ...rest}) => {
  const [length, setLength] = useState(0);

  const svgPath = useRef<SVGPathElement>(null);

  // TODO(dk): This is causing a pop in the animation when page loads.
  useEffect(() => {
    if (svgPath.current) {
      setLength(svgPath.current.getTotalLength());
    }
  }, []);

  return (
    <path
      className={cn('path', className)}
      d={d}
      ref={svgPath}
      style={{
        strokeDasharray: length || 400,
        strokeDashoffset: length || 400,
      }}
      {...rest}
    />
  );
};

export default function McodeLogo() {
  return (
    <svg className="mcode-logo" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 500 400">
      <Path className="line line-1" d="M250,350H50" />
      <Path className="line line-2" d="M250,363L50,200" />
      <Path className="line line-3" d="M250,350L50,50" />
      <Path className="line line-4" d="M250,350L450,50" />
      <Path className="line line-5" d="M250,363l200-163" />
      <Path className="line line-6" d="M250,350h200" />

      <path
        className="circle circle-0"
        d="M250,300c27.61,0,50,22.39,50,50s-22.39,50-50,50-50-22.39-50-50,22.39-50,50-50Z"
      />
      <path
        className="circle circle-1"
        d="M100,350c0,27.61-22.39,50-50,50S0,377.61,0,350s22.39-50,50-50,50,22.39,50,50Z"
      />
      <path
        className="circle circle-2"
        d="M100,200c0,27.61-22.39,50-50,50S0,227.61,0,200s22.39-50,50-50,50,22.39,50,50Z"
      />
      <path
        className="circle circle-3"
        d="M100,50c0,27.61-22.39,50-50,50S0,77.61,0,50,22.39,0,50,0s50,22.39,50,50Z"
      />
      <path
        className="circle circle-4"
        d="M500,50c0,27.61-22.39,50-50,50s-50-22.39-50-50S422.39,0,450,0s50,22.39,50,50Z"
      />
      <path
        className="circle circle-5"
        d="M500,200c0,27.61-22.39,50-50,50s-50-22.39-50-50,22.39-50,50-50,50,22.39,50,50Z"
      />
      <path
        className="circle circle-6"
        d="M500,350c0,27.61-22.39,50-50,50s-50-22.39-50-50,22.39-50,50-50,50,22.39,50,50Z"
      />
    </svg>
  );
}
