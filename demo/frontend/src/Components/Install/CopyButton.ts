import styled from 'styled-components';

export default styled.button`
  position: absolute;
  top: 10px;
  right: 10px;
  display: inline-flex;
  padding: 6px;
  border: solid 1px #fff;
  background-color: transparent;
  cursor: pointer;

  &:hover {
    border-color: transparent;
    box-shadow: 3px 3px 0 rgba(115, 0, 255, 1);
  }

  img {
    display: block;
  }
`;
