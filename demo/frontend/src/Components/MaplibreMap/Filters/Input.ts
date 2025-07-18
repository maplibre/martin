import styled from 'styled-components';

export default styled.input`
  width: 100%;
  height: 34px;
  padding: 0;
  margin: 0;
  background: transparent;
  -webkit-appearance: none;
  -moz-appearance: none;
  &:focus {
    outline: none;
  }
  &::-moz-focus-outer {
    border: 0;
  }
  &::-webkit-slider-runnable-track {
    height: 2px;
    border: none;
    border-radius: 1px;
    background-color: #c0c4d3;
  }
  &::-moz-range-track {
    height: 2px;
    border: none;
    border-radius: 1px;
    background-color: #c0c4d3;
  }
  &::-ms-track {
    height: 2px;
    border: none;
    border-radius: 1px;
    background-color: #c0c4d3;
  }
  &::-ms-fill-lower {
    background-color: #c0c4d3;
  }
  &::-ms-fill-upper {
    background-color: #c0c4d3;
  }
  &:focus::-ms-fill-lower {
    background-color: #c0c4d3;
  }
  &:focus::-ms-fill-upper {
    background-color: #c0c4d3;
  }
  &::-webkit-slider-thumb {
    width: 10px;
    height: 28px;
    border: none;
    border-radius: 2px;
    transform: translateY(calc(-50% + 1px));
    background-color: #ffffff;
    -webkit-appearance: none;
    &:hover {
      cursor: pointer;
    }
  }
  &::-moz-range-thumb {
    width: 10px;
    height: 28px;
    border: none;
    border-radius: 2px;
    background-color: #ffffff;
    -moz-appearance: none;
    &:hover {
      cursor: pointer;
    }
  }
  &::-ms-thumb {
    width: 10px;
    height: 28px;
    border: none;
    border-radius: 2px;
    background-color: #ffffff;
    &:hover {
      cursor: pointer;
    }
  }
  &::-ms-tooltip {
    display: none;
  }
`;
