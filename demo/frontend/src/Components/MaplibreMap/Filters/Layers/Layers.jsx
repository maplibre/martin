import layers from "../../../../config/layers";
import getColorsFromLayer from "../../../../utils/getColorsFromLayer";
import Description from "./Description";
import Layer from "./Layer";
import Legend from "./Legend";
import Title from "./Title";

const Layers = ({ visibleLayer, toggleLayer }) => {
    const toggleLayerHandler = (layerId) => () => {
        toggleLayer(layerId);
    };

    return layers.map((layer) => {
        const [fromColor, toColor] = getColorsFromLayer(
            layer.maplibreLayer,
            "fill-extrusion-color",
        );

        return (
            <Layer
                key={layer.id}
                onClick={toggleLayerHandler(layer.id)}
                $isLayerVisible={visibleLayer === layer.id}
            >
                <Title>{layer.title}</Title>
                {visibleLayer === layer.id && (
                    <>
                        <Description>{layer.description}</Description>
                        <Legend $fromColor={fromColor} $toColor={toColor} />
                    </>
                )}
            </Layer>
        );
    });
};

export default Layers;
