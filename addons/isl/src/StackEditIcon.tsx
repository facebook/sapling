/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

export function StackEditIcon(props: {slot?: 'start'}) {
  return (
    <svg
      width="64"
      height="64"
      viewBox="0 0 64 64"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      {...props}>
      <g clipPath="url(#clip0_108_2)">
        <mask
          id="mask0_108_2"
          style={{maskType: 'alpha'}}
          maskUnits="userSpaceOnUse"
          x="9"
          y="6"
          width="57"
          height="55">
          <path
            d="M21.5 40.5L52 6H9V61L63.5 58.5L66 24L47 43.5L31.5 51L21.5 40.5Z"
            fill="#D9D9D9"
          />
        </mask>
        <g mask="url(#mask0_108_2)">
          <path
            d="M15 18C15 19.6569 13.6569 21 12 21C10.3431 21 9 19.6569 9 18C9 16.3431 10.3431 15 12 15C13.6569 15 15 16.3431 15 18Z"
            fill="currentColor"
          />
          <path
            d="M15 32C15 33.6569 13.6569 35 12 35C10.3431 35 9 33.6569 9 32C9 30.3431 10.3431 29 12 29C13.6569 29 15 30.3431 15 32Z"
            fill="currentColor"
          />
          <path
            d="M15 46C15 47.6569 13.6569 49 12 49C10.3431 49 9 47.6569 9 46C9 44.3431 10.3431 43 12 43C13.6569 43 15 44.3431 15 46Z"
            fill="currentColor"
          />
          <path d="M20 18H53" stroke="currentColor" strokeWidth="3" />
          <path d="M11 18H14" stroke="currentColor" strokeWidth="3" />
          <path d="M20.0769 32L47 32" stroke="currentColor" strokeWidth="3" />
          <path d="M12 32H14.6923" stroke="currentColor" strokeWidth="3" />
          <path d="M20.1429 46L53 46" stroke="currentColor" strokeWidth="3" />
        </g>
        <path
          d="M34.9259 34.5L29 42.75L31.3704 45L40.4568 40.125M34.9259 34.5L55.0741 15L61 20.625L40.4568 40.125M34.9259 34.5L40.4568 40.125"
          stroke="currentColor"
          strokeWidth="3"
        />
      </g>
      <defs>
        <clipPath id="clip0_108_2">
          <rect width="64" height="64" fill="white" />
        </clipPath>
      </defs>
    </svg>
  );
}
