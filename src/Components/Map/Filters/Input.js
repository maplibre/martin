import styled from 'styled-components';

export default styled.input`
  width: 100%;
  height: 34px;
  padding: 0;
  margin: 0;
  background: transparent;

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
/* Thumb styles */
  &::-webkit-slider-thumb {
    width: 10px;
    height: 28px;
    border-radius: 2px;
    border-color: #fff;
    background-color: #fff;
  }
  &::-moz-range-thumb {
    width: 10px;
    height: 28px;
    border-radius: 2px;
    border-color: #fff;
    background-color: #fff;
  }
  &::-ms-thumb {
    width: 10px;
    height: 28px;
    border-radius: 2px;
    border-color: #fff;
    background-color: #fff;
  }
`;
