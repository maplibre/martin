import React, { PureComponent } from 'react';
import layers from '../../../../config/layers';
import getColorsFromLayer from '../../../../utils/getColorsFromLayer';

import Layer from './Layer';
import Title from './Title';
import Description from './Description';
import Legend from './Legend';

class Layers extends PureComponent {
  toggleLayerHandler = layerId => () => {
    this.props.toggleLayer(layerId);
  };

  render() {
    const { visibleLayer } = this.props;

    return (
      layers.map((layer) => {
        const isLayerVisible = visibleLayer === layer.id;
        const [fromColor, toColor] = getColorsFromLayer(layer.maplibreLayer, 'fill-extrusion-color');

        return (
          <Layer
            key={layer.id}
            onClick={this.toggleLayerHandler(layer.id)}
            isLayerVisible={isLayerVisible}
          >
            <Title>
              {layer.title}
            </Title>
            {isLayerVisible && (
              <>
                <Description>
                  {layer.description}
                </Description>
                <Legend
                  fromColor={fromColor}
                  toColor={toColor}
                />
              </>
            )}
          </Layer>
        );
      })
    );
  }
}

export default Layers;
