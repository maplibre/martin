import styled from 'styled-components';

export default styled.div<{ isLayerVisible: boolean }>`
  padding: 5px;
  border-radius: 5px;
  margin-bottom: 15px;

  color: ${({ isLayerVisible }) => (isLayerVisible ? '#DADFEE' : '#6C7495')};
  cursor: pointer;

  &:hover {
    background-color: #161626;
  }
`;
