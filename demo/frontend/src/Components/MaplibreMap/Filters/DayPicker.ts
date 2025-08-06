import styled from "styled-components";

export default styled.div`
  border-radius: 5px;
  margin-bottom: 10px;

  background-color: #161626;

  .rdp-wrapper {
    outline: none;
  }

  .rdp-caption_label {
    font-weight: bold;
    color: #dadfee;
    margin-left: 0.5em;
  }

  .rdp-button_previous {
    margin-right: 0.7em;
  }

  .rdp-button_next,
  .rdp-button_previous {
    width: 2.5em;
    height: 2.4em;
  }

  .rdp-weekday {
    color: #6C7495;
  }

  .rdp-range_middle {
    background-color: #2c0ea6 !important;
  }
  .rdp-range_start,
  .rdp-range_end {
    background-color: transparent !important;
    color: #2c0ea6 !important;
  }

  .rdp-day:not(
  .rdp-disabled):not(
  .rdp-selected):not(
  .rdp-disabled):hover {
    color: #000;
    border-radius: 2px;
  }

  .rdp-day {
    border-radius: 0 !important;
    outline: none;
  }
`;
