import styled from 'styled-components';

export default styled.button<{ $active: boolean }>`
  padding: 8px 14px;
  border: solid 1px ${(props) => (props.$active ? 'transparent' : '#fff')};
  font-family: inherit;
  font-size: 18px;
  color: #fff;
  background-color: transparent;
  box-shadow: ${(props) => (props.$active ? '3px 3px 0 rgba(115, 0, 255, 1)' : 'none')};
  cursor: pointer;
`;
