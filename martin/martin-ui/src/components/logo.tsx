import { useTheme } from 'next-themes';
import type { SVGProps } from 'react';

interface LogoProps extends Omit<SVGProps<SVGSVGElement>, 'width' | 'height'> {
  width?: number;
  height?: number;
}

const Logo = ({ width = 96, height = 48, className, ...props }: LogoProps) => {
  const { resolvedTheme } = useTheme();
  const color = resolvedTheme === 'dark' ? 'rgba(231, 163, 245, 0.70)' : 'rgba(93,44,228, 0.52)';
  return (
    <div className={className} style={{ height, width }}>
      <svg
        height="100%"
        viewBox="0 0 1310 898"
        width="100%"
        xmlns="http://www.w3.org/2000/svg"
        xmlnsXlink="http://www.w3.org/1999/xlink"
        {...props}
      >
        <title>{'Martin Tile Server'}</title>
        <defs>
          <filter
            filterUnits="objectBoundingBox"
            height="108.9%"
            id="a"
            width="104.4%"
            x="-2.2%"
            y="-2.2%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="108.6%"
            id="c"
            width="106.4%"
            x="-3.2%"
            y="-2.2%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="108.3%"
            id="e"
            width="111.2%"
            x="-5.6%"
            y="-2.1%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="108%"
            id="g"
            width="146.2%"
            x="-23.1%"
            y="-2%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="107.8%"
            id="i"
            width="121.7%"
            x="-10.8%"
            y="-1.9%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="109%"
            id="k"
            width="104.3%"
            x="-2.2%"
            y="-2.2%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="108.6%"
            id="m"
            width="106.1%"
            x="-3%"
            y="-2.2%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="108.3%"
            id="o"
            width="110.4%"
            x="-5.2%"
            y="-2.1%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="108%"
            id="q"
            width="117%"
            x="-8.5%"
            y="-2%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="107.8%"
            id="s"
            width="115.1%"
            x="-7.6%"
            y="-1.9%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="109%"
            id="u"
            width="104.2%"
            x="-2.1%"
            y="-2.3%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="108.7%"
            id="w"
            width="105.8%"
            x="-2.9%"
            y="-2.2%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="108.4%"
            id="y"
            width="108.4%"
            x="-4.2%"
            y="-2.1%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="108.1%"
            id="A"
            width="107.9%"
            x="-3.9%"
            y="-2%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="107.8%"
            id="C"
            width="107.5%"
            x="-3.7%"
            y="-2%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="109.1%"
            id="E"
            width="104.1%"
            x="-2%"
            y="-2.3%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="108.7%"
            id="G"
            width="105.4%"
            x="-2.7%"
            y="-2.2%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="108.4%"
            id="I"
            width="105.3%"
            x="-2.7%"
            y="-2.1%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="108.1%"
            id="K"
            width="105.1%"
            x="-2.6%"
            y="-2%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="107.8%"
            id="M"
            width="105%"
            x="-2.5%"
            y="-2%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="109.1%"
            id="O"
            width="103.9%"
            x="-2%"
            y="-2.3%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="108.8%"
            id="Q"
            width="104%"
            x="-2%"
            y="-2.2%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="108.4%"
            id="S"
            width="103.9%"
            x="-2%"
            y="-2.1%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="108.1%"
            id="U"
            width="103.8%"
            x="-1.9%"
            y="-2%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <filter
            filterUnits="objectBoundingBox"
            height="107.9%"
            id="W"
            width="103.7%"
            x="-1.9%"
            y="-2%"
          >
            <feOffset dy={5} in="SourceAlpha" result="shadowOffsetOuter1" />
            <feGaussianBlur in="shadowOffsetOuter1" result="shadowBlurOuter1" stdDeviation={2} />
            <feComposite
              in="shadowBlurOuter1"
              in2="SourceAlpha"
              operator="out"
              result="shadowBlurOuter1"
            />
            <feColorMatrix
              in="shadowBlurOuter1"
              values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.5 0"
            />
          </filter>
          <path d="M159.673 882.935 1.729 1006.155l209.318-79.02L384.232 804.94z" id="b" />
          <path d="M140.655 948.856 34.267 1075.539l145.651-83.296 121.628-125.66z" id="d" />
          <path d="m119.17 1015.411-54.83 130.147 81.984-87.573 70.07-129.123z" id="f" />
          <path d="m95.47 1083.473-3.273 133.61 18.316-91.85 18.514-132.587z" id="h" />
          <path d="m69.577 1152.17 48.283 137.073-45.35-96.128-33.044-136.05z" id="j" />
          <path d="M520.082 776.307 298.47 895.249l157.762-82.483 236.852-117.918z" id="l" />
          <path d="M442.848 811.686 272.793 934.092l94.094-86.76L552.182 725.95z" id="n" />
          <path d="m368.287 842.147-118.498 125.87 30.427-91.037 133.738-124.846z" id="p" />
          <path d="M287.37 879.667 220.428 1009l-33.24-95.314 82.181-128.309z" id="r" />
          <path d="m209.294 912.27-15.384 132.797-96.908-99.592 30.624-131.773z" id="t" />
          <path d="M877.474 672.455 592.195 787.12 698.4 701.173l300.52-113.64z" id="v" />
          <path d="M747.848 672.375 514.126 790.504l42.537-90.224 248.963-117.105z" id="x" />
          <path d="M614.927 671.66 432.762 793.252l-21.13-94.5 197.406-120.569z" id="z" />
          <path d="M485.053 670.945 354.445 796l-84.798-98.778 145.85-124.032z" id="B" />
          <path d="m347.312 674.983-79.052 128.52-148.464-103.056 94.292-127.495z" id="D" />
          <path d="M1237.12 567.18 888.175 677.57l54.648-89.41 364.187-109.364z" id="F" />
          <path d="M1052.704 532.429 755.314 646.28l-9.019-93.687 312.63-112.827z" id="H" />
          <path d="M861.568 501.172 615.735 618.487l-72.687-97.964 261.073-116.291z" id="J" />
          <path d="M676.495 467.14 482.219 587.918 345.865 485.677 555.38 365.922z" id="L" />
          <path d="M488.375 433.107 345.656 557.349 145.634 450.83l157.96-123.218z" id="N" />
          <path d="m1594.816 462.61-412.614 106.11 3.09-92.874 427.855-105.086z" id="P" />
          <path d="M1354.544 395.259 993.487 504.833l-60.577-97.15 376.297-108.55z" id="R" />
          <path d="m1111.225 327.908-309.5 113.038L677.48 339.518l324.74-112.014z" id="T" />
          <path d="M870.953 260.558 613.009 377.059 425.1 271.354l273.183-115.477z" id="V" />
          <path d="M687.665 109.913 481.278 229.878 229.7 119.896 451.326.955z" id="X" />
        </defs>
        <g fill="none" fillRule="evenodd">
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#a)" xlinkHref="#b" />
            <path
              d="M159.77 883.113 2.89 1005.503l208.042-78.53 172.025-121.378-223.187 77.518Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#c)" xlinkHref="#d" />
            <path
              d="m140.782 949.015-105.64 125.793 144.633-82.705 120.79-124.793-159.783 81.705Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#e)" xlinkHref="#f" />
            <path
              d="M119.337 1015.53 64.96 1144.606l81.189-86.716 69.525-128.117-96.336 85.759Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#g)" xlinkHref="#h" />
            <path
              d="m95.67 1083.51-3.217 131.263 17.862-89.568 18.258-130.753-32.903 89.059Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#i)" xlinkHref="#j" />
            <path
              d="m41.04 1062.7 31.65 130.33 43.682 92.59-46.983-133.384-.002-.006-28.347-89.53Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#k)" xlinkHref="#l" />
            <path
              d="m520.172 776.485-189.1 101.493 125.07-65.39L676.624 702.82l-156.451 73.665Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#m)" xlinkHref="#n" />
            <path
              d="m442.728 811.526 105.634-82.834-181.339 118.787L275.357 932l167.37-120.474Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#o)" xlinkHref="#p" />
            <path
              d="m368.122 842.031 45.142-88.98-132.858 123.992-30.139 90.174 117.855-125.186Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#q)" xlinkHref="#r" />
            <path
              d="m287.16 879.637-17.893-93.73-81.858 127.805 33.055 94.784 66.696-128.859Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#s)" xlinkHref="#t" />
            <path
              d="M209.086 912.332 127.73 814.14 97.22 945.414l96.538 99.212 15.327-132.294Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#u)" xlinkHref="#v" />
            <path
              d="m877.379 672.278 120.115-83.993L698.526 701.33l-105.17 85.108 284.023-114.16Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#w)" xlinkHref="#x" />
            <path
              d="m747.71 672.22 57.367-88.565L556.814 700.43l-42.26 89.632L747.71 672.22Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#y)" xlinkHref="#z" />
            <path
              d="m614.72 671.557-5.861-93.03-197 120.32 21.035 94.077L614.72 671.557Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#A)" xlinkHref="#B" />
            <path
              d="m484.79 670.92-69.33-97.436-145.53 123.761 84.529 98.465L484.79 670.92Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#C)" xlinkHref="#D" />
            <path
              d="m347.047 675.032-132.92-101.798L120.08 700.4l148.122 102.817 78.846-128.185Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#E)" xlinkHref="#F" />
            <path
              d="m1237.002 567.009 69.456-87.84-363.505 109.16-54.33 88.888 348.379-110.208Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#G)" xlinkHref="#H" />
            <path
              d="m1052.513 532.288 6.192-92.23-312.196 112.67 8.98 93.271 297.024-113.711Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#I)" xlinkHref="#J" />
            <path
              d="m861.284 501.086-57.244-96.599-260.682 116.117 72.44 97.632 245.486-117.15Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#K)" xlinkHref="#L" />
            <path
              d="M676.154 467.116 555.36 366.165 346.23 485.699l136 101.977 193.925-120.56Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#M)" xlinkHref="#N" />
            <path
              d="m488.031 433.141-184.42-105.289L146.001 450.8l199.628 106.309L488.03 433.14Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#O)" xlinkHref="#P" />
            <path
              d="m1594.644 462.447 18.245-91.418-427.401 104.975-3.077 92.456 412.233-106.013Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#Q)" xlinkHref="#R" />
            <path
              d="m1354.264 395.135-45.165-95.764-375.879 108.43 60.355 96.796 360.689-109.462Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#S)" xlinkHref="#T" />
            <path
              d="m1110.848 327.833-108.674-100.1L677.886 339.59l123.877 101.13 309.085-112.887Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#U)" xlinkHref="#V" />
            <path
              d="M870.524 260.532 698.266 156.101 425.552 271.38l187.467 105.455 257.505-116.303Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
          <g transform="translate(-307 -22)">
            <use fill="#000" fillOpacity={0.5} filter="url(#W)" xlinkHref="#X" />
            <path
              d="M687.231 109.934 451.333 1.178l-221.176 118.7 251.108 109.777 205.966-119.721Z"
              fill={color}
              stroke="#000"
              strokeLinejoin="bevel"
              strokeWidth={0.4}
            />
          </g>
        </g>
      </svg>
    </div>
  );
};
export default Logo;
